# Syncing to a new upstream release

The steps that move the port from the pinned upstream release to a newer one. Follow them in order,
in one change. The pin lives in `Cargo.toml` under `[package.metadata.upstream]`; the module-to-file
map is in `architecture.md`.

1. `git -C <checkout> fetch --tags`, then `scripts/upstream.sh` (or `.ps1`). It prints the pin, the
   newest tag, the commits and the files changed under `trafilatura/` and `tests/`.
2. For each changed upstream file, find its Rust module in the module map. Read the upstream diff
   (`scripts/upstream.sh --patch` covers `trafilatura/` and `tests/`) and port each hunk to the
   function that carries its `upstream:` trace comment.
3. Port the changed upstream tests to their Rust tests by name. A test upstream deleted is deleted
   here.
4. After porting token, selector, html-processing or output changes, regenerate the generated test
   data so it encodes the new upstream (`parity-harness.md`, Generated test data): `gen_xpath_cases`,
   `gen_html_processing_cases`, `gen_main_extractor_cases`, `gen_xml_output_cases` and both outputs of
   `gen_url`. The selector generator fails with "token table out of date" until each new token has a
   value in its table. After a `main_extractor.py` change, run the `report()` of
   `gen_main_extractor_cases` and add a case for each line it lists. `gen_decimal` holds the
   Unicode digit data of the oracle image's Python, not upstream's: rerun it when that image's Python
   or its Unicode data changes. The size and digest of `html.entities.html5` recorded in
   `entity_table_matches_python` (`src/pyhtml.rs`) come from that Python too: rerun the snippet in its
   comment when it changes, and when a `web_atoms` bump fails that test, diff the table before
   updating the values.
5. Read upstream's `settings.cfg` diff as well as `settings.py`: defaults such as `MAX_FILE_SIZE`
   live there. Check OI-5 in `phase-plan.md`: a merged upstream parser switch changes D-2 and the
   fixtures. Check the version bumps of upstream's own dependencies in `pyproject.toml` (`htmldate`, `courlan`,
   `charset_normalizer`, `lxml`): a dependency release can change extraction output with no change in
   trafilatura itself. Diff the ported parts of that package too. `courlan.rs` ports the version in
   `tools/parity/uv.lock`, so a new one means diffing `courlan/urlutils.py`, `courlan/filters.py` and
   `courlan/clean.py` between the two versions and rerunning `gen_url`. A new `tld` release changes
   `TLD_CASES` and `courlan/tld_tables.rs` when rerun: run the tests to see whether `psl`, whose suffix
   list is its own release's, still agrees, and update `psl` in `Cargo.lock` (`cargo update -p psl`) when a
   newer list closes a difference (D-18, `docs/dependencies.md`).
6. Regenerate the parity fixtures from upstream Python at the new tag: run
   `scripts/fixtures.sh --generate --pack` (details in `parity-harness.md`). The new `version` names
   the new archive and directory (`fixtures-<version>.tar.zst`, `.temp/fixtures/<version>`); `git rm`
   the old version's archive in the same change, add the new one, and run `scripts/test.sh`. A
   fixture that changed without a matching upstream change is a parser or port difference: fix it,
   or record it in `deviations.md`.
7. Re-check every `deviations.md` row against the new release: upstream may have fixed what a row
   works around, and the row then goes.
8. Update `version` and `commit` in `[package.metadata.upstream]` in the same change as the port.
