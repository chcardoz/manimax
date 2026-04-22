import manim_rs
from manim_rs import _rust


def test_rust_extension_imports() -> None:
    assert _rust.__build_probe().startswith("manim_rs._rust")
    assert manim_rs.__version__ == "0.0.0"
