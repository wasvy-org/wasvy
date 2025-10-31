from typing import TypeVar, Generic, Union, Optional, Protocol, Tuple, List, Any, Self
from types import TracebackType
from enum import Flag, Enum, auto
from dataclasses import dataclass
from abc import abstractmethod
import weakref

from ..types import Result, Ok, Err, Some


class Schedule(Enum):
    UPDATE = 0


@dataclass
class QueryFor_Ref:
    value: str


@dataclass
class QueryFor_Mut:
    value: str


@dataclass
class QueryFor_With:
    value: str


@dataclass
class QueryFor_Without:
    value: str


QueryFor = Union[QueryFor_Ref, QueryFor_Mut, QueryFor_With, QueryFor_Without]


class System:
    """
    An interface with which to define a new system for the host
    
    Usage:
    1. Construct a new system, giving it a unique name
    2. Add system-params by calling 0 or more add-* methods
    3. Order the system relative to others
    4. Add the system to a schedule
    """
    
    def __init__(self, name: str) -> None:
        """
        Constructs a new system. Use the same name as exported in
        the guest world, otherwise the host won't be able to find it.
        """
        raise NotImplementedError

    def add_commands(self) -> None:
        """
        Adds a commands system-param
        """
        raise NotImplementedError
    def add_query(self, query: List[QueryFor]) -> None:
        """
        Adds a query system-param
        """
        raise NotImplementedError
    def after(self, other: Self) -> None:
        """
        Schedules this system be run after another system
        """
        raise NotImplementedError
    def before(self, other: Self) -> None:
        """
        Schedules this system be run before another system
        """
        raise NotImplementedError
    def __enter__(self) -> Self:
        """Returns self"""
        return self
                                
    def __exit__(self, exc_type: type[BaseException] | None, exc_value: BaseException | None, traceback: TracebackType | None) -> bool | None:
        """
        Release this resource.
        """
        raise NotImplementedError


class App:
    """
    A mod, similar to bevy::App
    """
    
    def __init__(self) -> None:
        """
        Construct an new App: an interface through which mods may interact with the bevy world.
        
        Each mod may only do this once inside its setup function call. Attempting to do this
        twice or outside setup will trap.
        """
        raise NotImplementedError

    def add_systems(self, schedule: Schedule, systems: List[System]) -> None:
        """
        Adds systems to the mod
        """
        raise NotImplementedError
    def __enter__(self) -> Self:
        """Returns self"""
        return self
                                
    def __exit__(self, exc_type: type[BaseException] | None, exc_value: BaseException | None, traceback: TracebackType | None) -> bool | None:
        """
        Release this resource.
        """
        raise NotImplementedError


class Commands:
    """
    A commands system param
    """
    
    def spawn(self, components: List[Tuple[str, str]]) -> None:
        raise NotImplementedError
    def __enter__(self) -> Self:
        """Returns self"""
        return self
                                
    def __exit__(self, exc_type: type[BaseException] | None, exc_value: BaseException | None, traceback: TracebackType | None) -> bool | None:
        """
        Release this resource.
        """
        raise NotImplementedError


class Component:
    
    def get(self) -> str:
        """
        Gets the value of a component
        """
        raise NotImplementedError
    def set(self, value: str) -> None:
        """
        Sets the value of a component
        
        Traps if this component was not declared as mutable
        """
        raise NotImplementedError
    def __enter__(self) -> Self:
        """Returns self"""
        return self
                                
    def __exit__(self, exc_type: type[BaseException] | None, exc_value: BaseException | None, traceback: TracebackType | None) -> bool | None:
        """
        Release this resource.
        """
        raise NotImplementedError


class Query:
    """
    A query system param
    """
    
    def iter(self) -> Optional[List[Component]]:
        """
        Evaluates and returns the next query results
        """
        raise NotImplementedError
    def __enter__(self) -> Self:
        """Returns self"""
        return self
                                
    def __exit__(self, exc_type: type[BaseException] | None, exc_value: BaseException | None, traceback: TracebackType | None) -> bool | None:
        """
        Release this resource.
        """
        raise NotImplementedError



