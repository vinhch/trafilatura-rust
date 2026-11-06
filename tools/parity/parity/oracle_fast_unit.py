"""Fast-mode oracle for the ported tests: upstream's test code run with extract forced to fast mode, and the fast-mode values the ported cases assert."""

import ast
import functools
import importlib.util
import os
import pathlib
import sys
import traceback
import types

# The inputs of upstream's test_extract_div_keeps_inline_link
_DIV_FOLLOWING = (
    " before planting the seeds. They recorded the width of each plot, checked the soil, "
    "and marked every corner with a wooden stake. The measurements will help the volunteers "
    "leave enough space between rows when they return to plant beans next week."
)
_DIV_DOCUMENT = (
    '<html><body><div>The garden team measured <a href="https://example.org/plots">three plots</a>'
    f"{_DIV_FOLLOWING}</div></body></html>"
)
# The input of upstream's test_lb_only_subtree_stays_one_paragraph
_LB_DOCUMENT = (
    '<html><body><div class="entry-content">'
    "<br/>First line of text long enough to be kept by the extractor here."
    "<br/>Second line of text long enough to be kept by the extractor here."
    "</div></body></html>"
)


def _pytest_stub() -> types.ModuleType:
    """Stand-in for pytest: parametrize records its cases on the test function, the rest does nothing."""

    class Dyn:
        def __getattr__(self, name):
            return Dyn()

        def __call__(self, *args, **kwargs):
            if len(args) == 1 and callable(args[0]) and not kwargs:
                return args[0]
            return Dyn()

    def parametrize(names, values, ids=None):
        def deco(f):
            f.__dict__.setdefault("_params", []).append((names, values))
            return f

        return deco

    def fixture(*args, **kwargs):
        if len(args) == 1 and callable(args[0]) and not kwargs:
            return args[0]
        return lambda f: f

    stub = types.ModuleType("pytest")
    stub.mark = Dyn()
    stub.mark.parametrize = parametrize
    stub.fixture = fixture
    stub.param = lambda *args, **kwargs: args
    return stub


@functools.cache
def _load(file_name: str) -> tuple[types.ModuleType, str]:
    """Upstream's tests/<file_name> as a module with `extract` forced to fast mode, and its path."""
    sys.modules["pytest"] = _pytest_stub()
    path = os.path.join(os.environ["TRAFILATURA_UPSTREAM"], "tests", file_name)
    spec = importlib.util.spec_from_file_location(
        f"upstream_{file_name.removesuffix('.py')}", path
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    orig = mod.extract

    def fast_extract(*args, **kwargs):
        kwargs["fast"] = True
        return orig(*args, **kwargs)

    mod.extract = fast_extract
    return mod, path


def _unit_module() -> types.ModuleType:
    return _load("unit_tests.py")[0]


def _parameter_sets(f) -> list[dict]:
    """The keyword arguments of each case of an upstream test, expanded as pytest expands parametrize."""
    cases = [{}]
    for names, values in getattr(f, "_params", []):
        keys = (
            [n.strip() for n in names.split(",")]
            if isinstance(names, str)
            else list(names)
        )
        cases = [
            {**base, **dict(zip(keys, v if len(keys) > 1 else (v,)))}
            for base in cases
            for v in values
        ]
    return cases


def _run_unit_test(name: str) -> str:
    mod, path = _load("unit_tests.py")
    f = getattr(mod, name)
    lines = []
    for kw in _parameter_sets(f):
        label = f"{name} {kw}"
        try:
            f(**kw)
        except Exception as e:  # noqa: BLE001 - every failure is reported with its type
            tb = [
                fr
                for fr in traceback.extract_tb(e.__traceback__)
                if fr.filename == path
            ]
            line = tb[-1].lineno if tb else "?"
            text = tb[-1].line if tb else ""
            lines.append(
                f"FAIL {label} line {line}: {text} :: {type(e).__name__} {str(e)[:300]}"
            )
        else:
            lines.append(f"PASS {label}")
    return "\n".join(lines)


class _AssertToCheck(ast.NodeTransformer):
    """Replaces each assert with a call to `_chk` that takes the assert's file line, so the test runs on past a failure."""

    def __init__(self, offset: int):
        self.offset = offset

    def visit_Assert(self, node):
        call = ast.Call(
            func=ast.Name(id="_chk", ctx=ast.Load()),
            args=[node.test, ast.Constant(value=node.lineno + self.offset)],
            keywords=[],
        )
        return ast.copy_location(ast.Expr(value=call), node)


def _recorded_test(lines: list[str], path: str, name: str):
    start = next(i for i, line in enumerate(lines) if line.startswith(f"def {name}("))
    end = start + 1
    while end < len(lines) and (lines[end] == "" or lines[end][0].isspace()):
        end += 1
    text = "\n".join(lines[start:end]).rstrip() + "\n"
    tree = ast.parse(text)
    fn = tree.body[0]
    fn.name = "_recorded"
    fn.decorator_list = []
    tree = _AssertToCheck(start).visit(tree)
    ast.fix_missing_locations(tree)
    return compile(tree, path, "exec")


def _run_realworld_test(name: str) -> str:
    mod, path = _load("realworld_tests.py")
    lines = pathlib.Path(path).read_text(encoding="utf-8").split("\n")
    f = getattr(mod, name)
    failed: list[int] = []

    def check(ok: bool, lineno: int) -> None:
        if not ok:
            failed.append(lineno)

    ns = dict(mod.__dict__)
    ns["_chk"] = check
    exec(_recorded_test(lines, path, name), ns)  # noqa: S102 - runs the test's own source with its asserts replaced
    out = []
    for kw in _parameter_sets(f):
        failed.clear()
        try:
            ns["_recorded"](**kw)
            status = "DONE"
        except Exception as e:  # noqa: BLE001 - every failure is reported with its type
            status = f"EXC {type(e).__name__}: {str(e)[:200]}"
        out.append(f"CASE {name} {kw} {status} failed_assert_lines={list(failed)}")
    return "\n".join(out)


def _div_keeps_markdown() -> str:
    return _unit_module().extract(
        _DIV_DOCUMENT, output_format="markdown", include_links=True
    )


def _div_keeps_xml() -> str:
    return _unit_module().extract(
        _DIV_DOCUMENT, output_format="xml", include_links=True
    )


def _lb_only_xml() -> str:
    mod = _unit_module()
    return mod.extract(_LB_DOCUMENT, config=mod.ZERO_CONFIG, output_format="xml")


# Fast-mode values the ported tests assert where upstream's own assertion is the fast=False value.
_VALUE_CASES = {
    "test_extract_div_keeps_inline_link_markdown": _div_keeps_markdown,
    "test_extract_div_keeps_inline_link_xml": _div_keeps_xml,
    "test_lb_only_subtree_stays_one_paragraph_xml": _lb_only_xml,
}


def run(case: str) -> str:
    """The fast-mode output of a unit-test case: its value if it is a value case, else one PASS or FAIL line per parameter set."""
    if case in _VALUE_CASES:
        return _VALUE_CASES[case]()
    return _run_unit_test(case)


def run_realworld(case: str) -> str:
    """One line per parameter set of a realworld test, with the assert lines that fail in fast mode."""
    return _run_realworld_test(case)
