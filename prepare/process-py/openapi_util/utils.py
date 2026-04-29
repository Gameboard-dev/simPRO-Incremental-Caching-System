from __future__ import annotations

import re
from typing import Callable, Iterable, TypeVar

T = TypeVar("T")


def partition(
    iterable: Iterable[T],
    predicate: Callable[[T], bool],
) -> tuple[list[T], list[T]]:
    truthy: list[T] = []
    falsey: list[T] = []

    for item in iterable:
        (truthy if predicate(item) else falsey).append(item)

    return truthy, falsey


def separate_into_words(string: str) -> list[str]:
    if not string:
        return []

    words: list[str] = []
    current = string[0]

    for char in string[1:]:
        previous = current[-1]

        if char.isupper() and not previous.isupper():
            words.append(current)
            current = char
        else:
            current += char

    words.append(current)
    return words


def to_snake(name: str) -> str:
    name = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1_\2", name)
    name = re.sub(r"ID$", "Id", name)
    return re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", name).lower()


def capitalize(word: str) -> str:
    return word[:1].upper() + word[1:]