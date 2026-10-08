"""Tests for reciprocal rank fusion of search result lists."""

from ranking import rrf


def test_item_in_both_lists_wins():
    lexical = ["a", "b", "c"]
    semantic = ["c", "d", "a"]
    fused = rrf([lexical, semantic], k=60)
    assert fused[0] == "a"


def test_weights_change_the_order():
    fused = rrf([["x", "y"], ["y", "x"]], k=60, weights=[1.0, 3.0])
    assert fused == ["y", "x"]
