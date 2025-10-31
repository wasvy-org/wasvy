"""
An example world for the component to target.
"""
from typing import TypeVar, Generic, Union, Optional, Protocol, Tuple, List, Any, Self
from types import TracebackType
from enum import Flag, Enum, auto
from dataclasses import dataclass
from abc import abstractmethod
import weakref

from .types import Result, Ok, Err, Some
from .imports import app


class Example(Protocol):

    @abstractmethod
    def spin_cube(self, query: app.Query) -> None:
        """
        An example system
        """
        raise NotImplementedError

    @abstractmethod
    def my_system(self, commands: app.Commands, query: app.Query) -> None:
        """
        Another system
        """
        raise NotImplementedError

    @abstractmethod
    def setup(self) -> None:
        """
        This function is called once on startup for each WASM component (Not Bevy component).
        """
        raise NotImplementedError

