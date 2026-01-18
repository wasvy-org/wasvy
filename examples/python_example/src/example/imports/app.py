from typing import TypeVar, Generic, Union, Optional, Protocol, Tuple, List, Any, Self
from types import TracebackType
from enum import Flag, Enum, auto
from dataclasses import dataclass
from abc import abstractmethod
import weakref

from ..types import Result, Ok, Err, Some



@dataclass
class Schedule_ModStartup:
    pass


@dataclass
class Schedule_PreUpdate:
    pass


@dataclass
class Schedule_Update:
    pass


@dataclass
class Schedule_PostUpdate:
    pass


@dataclass
class Schedule_FixedPreUpdate:
    pass


@dataclass
class Schedule_FixedUpdate:
    pass


@dataclass
class Schedule_FixedPostUpdate:
    pass


@dataclass
class Schedule_Custom:
    value: str


Schedule = Union[Schedule_ModStartup, Schedule_PreUpdate, Schedule_Update, Schedule_PostUpdate, Schedule_FixedPreUpdate, Schedule_FixedUpdate, Schedule_FixedPostUpdate, Schedule_Custom]



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
    An interface with which to define a new system for the host.
    
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
    This is an interface (similar to bevy::App) through which mods may interact with the Bevy App.
    
    To access this, make sure to import the 'guest' world and implement `setup`.
    """
    
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


class Entity:
    """
    An identifier for an entity.
    """
    
    def __enter__(self) -> Self:
        """Returns self"""
        return self
                                
    def __exit__(self, exc_type: type[BaseException] | None, exc_value: BaseException | None, traceback: TracebackType | None) -> bool | None:
        """
        Release this resource.
        """
        raise NotImplementedError


class EntityCommands:
    """
    A list of commands that will be run to modify an `entity`.
    """
    
    def id(self) -> Entity:
        """
        Returns the identifier for this entity
        """
        raise NotImplementedError
    def insert(self, bundle: List[Tuple[str, str]]) -> None:
        """
        Adds a `bundle` of components to the entity.
        
        This will overwrite any previous value(s) of the same component type.
        """
        raise NotImplementedError
    def remove(self, bundle: List[str]) -> None:
        """
        Removes a Bundle of components from the entity if it exists.
        """
        raise NotImplementedError
    def despawn(self) -> None:
        """
        Despawns the entity.
        
        This will emit a warning if the entity does not exist.
        """
        raise NotImplementedError
    def try_despawn(self) -> None:
        """
        Despawns the entity.
        
        Unlike `despawn`, this will not emit a warning if the entity does not exist.
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
    A `command` queue system param to perform structural changes to the world.
    
    Since each command requires exclusive access to the world,
    all queued commands are automatically applied in sequence.
    
    Each command can be used to modify the world in arbitrary ways:
    - spawning or despawning entities
    - inserting components on new or existing entities
    - etc.
    """
    
    def spawn_empty(self) -> EntityCommands:
        """
        Spawns a new empty `entity` and returns its corresponding `entity-commands`.
        """
        raise NotImplementedError
    def spawn(self, bundle: List[Tuple[str, str]]) -> EntityCommands:
        """
        Spawns a new `entity` with the given components
        and returns the entity's corresponding `entity-commands`.
        """
        raise NotImplementedError
    def entity(self, entity: Entity) -> EntityCommands:
        """
        Returns the `entity-commands` for the given `entity`.
        
        This method does not guarantee that commands queued by the returned `entity-commands`
        will be successful, since the entity could be despawned before they are executed.
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


class QueryResult:
    """
    A query system param
    """
    
    def entity(self) -> Entity:
        """
        Returns the entity id for the query
        """
        raise NotImplementedError
    def component(self, index: int) -> Component:
        """
        Gets the component at the specified index. Order is the same as declared
        during setup. Query filters do not count as components.
        
        So for example:
        
        ```
        spin_cube.add_query(&[
        QueryFor::Mut("A"),     // component index 0
        QueryFor::With("B"),    // none
        QueryFor::Ref("C"),     // component index 1
        QueryFor::Without("D"), // none
        ]);
        ```
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
    
    def iter(self) -> Optional[QueryResult]:
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



