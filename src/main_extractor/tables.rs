//! The table handlers of upstream `main_extractor.py`.

use std::collections::{HashMap, HashSet};

use crate::context::Ctx;
use crate::dom::{LiveIter, NodeId, Tree};
use crate::error::{ExtractError, Rejection, TreeLimit};
use crate::html_processing::{handle_textnode, has_content, process_node, tick};
use crate::pystr;
use crate::settings::{Focus, INLINE_CONSUMING};
use crate::utils::text_chars_test;
use crate::xml::separates_inline;

use super::{
    MAX_OUTPUT_NODES, MAX_SPAN, TABLE_ELEMS, append_block, define_newelem, handle_lists,
    handle_textelem, in_tags,
};

/// Determines the cell element type and mints a new element; fails with
/// `Rejection::TreeTooLarge(TreeLimit::TableCells)` once table construction has already minted
/// `budget` cells in this tree.
// upstream: main_extractor.py::define_cell_type
fn define_cell_type(
    tree: &mut Tree,
    is_header: bool,
    budget: usize,
) -> Result<NodeId, ExtractError> {
    // deviation: D-9
    if tree.table_cells >= budget {
        return Err(ExtractError::Rejected(Rejection::TreeTooLarge(
            TreeLimit::TableCells,
        )));
    }
    tree.table_cells = tree.table_cells.saturating_add(1);
    let cell = tree.new_element("cell");
    if is_header {
        tree.set(cell, "role", "head");
    }
    Ok(cell)
}

/// A `row` under construction with its cell count; `Tree::child_count` walks the children, which
/// would make a row of n cells cost n squared.
struct Row {
    id: NodeId,
    len: usize,
}

impl Row {
    fn new(tree: &mut Tree) -> Self {
        Self {
            id: tree.new_element("row"),
            len: 0,
        }
    }

    fn push(&mut self, tree: &mut Tree, cell: NodeId) {
        tree.append(self.id, cell);
        self.len = self.len.saturating_add(1);
    }
}

/// Parses the `colspan` or `rowspan` of a cell: 1 by default and for a value that is not a decimal
/// number, at most `MAX_SPAN`. More than `MAX_STR_DIGITS` digits is `Rejection::InvalidValue`,
/// where upstream's `int()` raises `ValueError`.
// upstream: main_extractor.py::_span
pub fn span(tree: &Tree, cell: NodeId, attr: &str) -> Result<usize, ExtractError> {
    let value = tree.get(cell, attr).unwrap_or("1");
    // isdecimal, not isdigit: int() rejects the superscripts isdigit() admits
    if !pystr::is_decimal(value) {
        return Ok(1);
    }
    if value.chars().count() > pystr::MAX_STR_DIGITS {
        return Err(ExtractError::Rejected(Rejection::InvalidValue));
    }
    // capped at every step: exact below `MAX_SPAN`, `MAX_SPAN` above it
    Ok(value
        .chars()
        .filter_map(pystr::decimal_value)
        .fold(0usize, |acc, digit| {
            let digit = usize::try_from(digit).unwrap_or(0);
            acc.saturating_mul(10).saturating_add(digit).min(MAX_SPAN)
        }))
}

/// Inserts empty placeholder cells for the columns a rowspan still occupies at the current
/// position of the row.
// upstream: main_extractor.py::_flush_rowspan_phantoms
fn flush_rowspan_phantoms(
    tree: &mut Tree,
    rowspan_map: &mut HashMap<usize, usize>,
    newrow: &mut Row,
    budget: usize,
) -> Result<(), ExtractError> {
    while let Some(remaining) = rowspan_map.get_mut(&newrow.len) {
        let col = newrow.len;
        *remaining = remaining.saturating_sub(1);
        if *remaining == 0 {
            rowspan_map.remove(&col);
        }
        let phantom = define_cell_type(tree, false, budget)?;
        newrow.push(tree, phantom);
    }
    Ok(())
}

/// Closes a row: trailing rowspan placeholders, padding to `max_cols`, and the row is kept only
/// when a cell holds something.
// upstream: main_extractor.py::_finalize_row
fn finalize_row(
    tree: &mut Tree,
    newtable: NodeId,
    newrow: &mut Row,
    rowspan_map: &mut HashMap<usize, usize>,
    max_cols: usize,
    budget: usize,
) -> Result<(), ExtractError> {
    flush_rowspan_phantoms(tree, rowspan_map, newrow, budget)?;
    let filled = tree.children(newrow.id).any(|cell| {
        has_content(tree.node(cell).text.as_deref()) || tree.first_child(cell).is_some()
    });
    // padding cells are empty, so they never change `filled`; a row that is dropped is not padded
    if !filled {
        return Ok(());
    }
    while newrow.len < max_cols {
        let padding = define_cell_type(tree, false, budget)?;
        newrow.push(tree, padding);
    }
    tree.append(newtable, newrow.id);
    Ok(())
}

/// Extracts the content of a source `td` or `th` into the new `cell`, rewiring inline and block
/// children.
// upstream: main_extractor.py::_fill_cell
fn fill_cell(
    tree: &mut Tree,
    new_child_elem: NodeId,
    cell: NodeId,
    nested_elems: &HashSet<NodeId>,
    ptags_with_div: &[&str],
    ctx: &Ctx<'_>,
) -> Result<(), ExtractError> {
    if tree.first_child(cell).is_none() {
        if let Some(processed_cell) = process_node(tree, cell, ctx) {
            let node = tree.node(processed_cell);
            let (text, tail) = (node.text.clone(), node.tail.clone());
            let node = tree.node_mut(new_child_elem);
            node.text = text;
            node.tail = tail;
        }
        return Ok(());
    }
    let node = tree.node(cell);
    let (text, tail) = (node.text.clone(), node.tail.clone());
    let node = tree.node_mut(new_child_elem);
    node.text = text;
    node.tail = tail;
    // renamed before the inner walk so that handle_formatting wraps orphan spans in a `p`
    tree.set_name(cell, "done");
    let mut walk = LiveIter::descendants(tree, cell, &[]);
    let mut visited = 0usize;
    while let Some(child) = walk.next(tree) {
        tick(&mut visited, ctx.call)?;
        if tree.tag(child) == Some("done") {
            continue;
        }
        if nested_elems.contains(&child) {
            // nested tables are left to the main loop, their tail moves to the cell holding them;
            // a nested table always has an enclosing table, so a missing one cannot happen and
            // counts as not nested
            if tree.tag(child) == Some("table")
                && tree
                    .iterancestors(child, &["table"])
                    .next()
                    .is_none_or(|outer| !nested_elems.contains(&outer))
            {
                append_block(tree, new_child_elem, None, child);
                tree.node_mut(child).tail = None;
            }
            continue;
        }
        if separates_inline(tree, child) && !text_chars_test(tree.node(child).tail.as_deref()) {
            let linebreak = tree.new_element("lb");
            tree.append(new_child_elem, linebreak);
            tree.set_name(child, "done");
            continue;
        }
        let processed_subchild = if in_tags(tree, child, TABLE_ELEMS) {
            // stray cell from malformed HTML
            tree.set_name(child, "cell");
            handle_textnode(tree, child, ctx, true, true)
        } else if in_tags(tree, child, INLINE_CONSUMING) {
            let processed = handle_textnode(tree, child, ctx, true, true);
            // handle_textnode drops inline wrappers (ref/hi/del) with children but no direct text
            // (`<ref><hi>link text</hi></ref>`): carry the subtree directly instead
            if processed.is_none() && tree.first_child(child).is_some() {
                Some(child)
            } else {
                processed
            }
        } else if tree.tag(child) == Some("list") {
            let processed_list = handle_lists(tree, child, ctx)?;
            // lists in cells only in recall mode: keeping them otherwise is noise
            let kept = processed_list.filter(|_| ctx.options.focus == Focus::Recall);
            append_block(tree, new_child_elem, kept, child);
            continue;
        } else {
            handle_textelem(tree, child, ptags_with_div, ctx)?
        };
        define_newelem(tree, processed_subchild, new_child_elem, true, ctx.call)?;
        tree.set_name(child, "done");
    }
    Ok(())
}

/// Processes a single table element.
// upstream: main_extractor.py::handle_table
pub fn handle_table(
    tree: &mut Tree,
    table_elem: NodeId,
    potential_tags: &[&str],
    ctx: &Ctx<'_>,
) -> Result<Option<NodeId>, ExtractError> {
    handle_table_limited(tree, table_elem, potential_tags, ctx, MAX_OUTPUT_NODES)
}

/// `handle_table` with the cell budget as a parameter, so tests can use a small one.
pub(super) fn handle_table_limited(
    tree: &mut Tree,
    table_elem: NodeId,
    potential_tags: &[&str],
    ctx: &Ctx<'_>,
    budget: usize,
) -> Result<Option<NodeId>, ExtractError> {
    let newtable = tree.new_element("table");
    let ptags_with_div: Vec<&str> = potential_tags.iter().copied().chain(["div"]).collect();

    tree.strip_tags(table_elem, &["thead", "tbody", "tfoot"]);

    // Everything inside a nested table: skipped in the cell walk without a `done` mark, so the
    // main loop can call handle_table on each nested table separately. A table inside one already
    // collected is covered by it.
    let mut nested_elems: HashSet<NodeId> = HashSet::new();
    let mut visited = 0usize;
    for nested_table in tree.iterdescendants(table_elem, &["table"]) {
        tick(&mut visited, ctx.call)?;
        if nested_elems.contains(&nested_table) {
            continue;
        }
        for node in tree.iter(nested_table, &[]) {
            tick(&mut visited, ctx.call)?;
            nested_elems.insert(node);
        }
    }

    // Direct children only, nested tables are left to the main loop. Orphan cells join the
    // current row but do not count for max_cols.
    let mut rows: Vec<Vec<NodeId>> = vec![Vec::new()];
    let mut captions: Vec<String> = Vec::new();
    let mut max_cols = 0usize;
    let children: Vec<NodeId> = tree.children(table_elem).collect();
    for elem in children {
        tick(&mut visited, ctx.call)?;
        if tree.tag(elem) == Some("tr") {
            let cells: Vec<NodeId> = tree
                .children(elem)
                .filter(|&cell| in_tags(tree, cell, TABLE_ELEMS))
                .collect();
            let mut width = 0usize;
            for &cell in &cells {
                tick(&mut visited, ctx.call)?;
                width = width.saturating_add(span(tree, cell, "colspan")?);
            }
            max_cols = max_cols.max(width);
            rows.push(cells);
        } else if in_tags(tree, elem, TABLE_ELEMS) {
            if let Some(row) = rows.last_mut() {
                row.push(elem);
            }
            continue;
        } else if tree.tag(elem) == Some("caption") {
            let pieces: Vec<&str> = tree.itertext_lxml(elem).collect();
            captions.push(pystr::strip(&pieces.join(" ")).to_string());
            // text consumed above, not to be emitted again, images are left to the main loop
            let mut walk = LiveIter::descendants(tree, elem, &[]);
            while let Some(sub) = walk.next(tree) {
                tick(&mut visited, ctx.call)?;
                if tree.tag(sub) == Some("graphic") {
                    tree.node_mut(sub).tail = None;
                } else {
                    tree.set_name(sub, "done");
                }
            }
        } else if tree.tag(elem) == Some("table") {
            continue;
        }
        tree.set_name(elem, "done");
    }
    let max_cols = max_cols.min(MAX_SPAN);

    for caption_text in captions.into_iter().filter(|caption| !caption.is_empty()) {
        let caption_cell = define_cell_type(tree, true, budget)?;
        tree.node_mut(caption_cell).text = Some(caption_text);
        let mut caption_row = Row::new(tree);
        caption_row.push(tree, caption_cell);
        finalize_row(
            tree,
            newtable,
            &mut caption_row,
            &mut HashMap::new(),
            max_cols,
            budget,
        )?;
    }

    let mut header_row_emitted = false;
    // column index to the rows still spanned from a rowspan cell
    let mut rowspan_map: HashMap<usize, usize> = HashMap::new();
    for cells in rows {
        tick(&mut visited, ctx.call)?;
        let mut newrow = Row::new(tree);
        let mut row_has_th = false;
        for cell in cells {
            tick(&mut visited, ctx.call)?;
            let is_header = tree.tag(cell) == Some("th") && !header_row_emitted;
            row_has_th = row_has_th || is_header;
            flush_rowspan_phantoms(tree, &mut rowspan_map, &mut newrow, budget)?;
            let new_child_elem = define_cell_type(tree, is_header, budget)?;
            let colspan = span(tree, cell, "colspan")?;
            // mark all spanned columns as occupied for the following rows
            let rowspan = span(tree, cell, "rowspan")?;
            if rowspan > 1 {
                for col in newrow.len..newrow.len.saturating_add(colspan) {
                    rowspan_map.insert(col, rowspan.saturating_sub(1));
                }
            }
            fill_cell(
                tree,
                new_child_elem,
                cell,
                &nested_elems,
                &ptags_with_div,
                ctx,
            )?;
            // keep empty cells so column positions stay aligned
            newrow.push(tree, new_child_elem);
            // inline colspan: pad with empty cells so the following rows align
            for _ in 1..colspan {
                let padding = define_cell_type(tree, is_header, budget)?;
                newrow.push(tree, padding);
            }
            tree.set_name(cell, "done");
        }
        finalize_row(
            tree,
            newtable,
            &mut newrow,
            &mut rowspan_map,
            max_cols,
            budget,
        )?;
        header_row_emitted = header_row_emitted || row_has_th;
    }
    Ok(tree.first_child(newtable).is_some().then_some(newtable))
}
