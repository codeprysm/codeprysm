"""Models module with data classes and business logic."""

from dataclasses import dataclass, field
from enum import Enum


class Status(Enum):
    """Enumeration for task status."""

    PENDING = "pending"
    IN_PROGRESS = "in_progress"
    COMPLETED = "completed"
    CANCELLED = "cancelled"


@dataclass
class Task:
    """Task data class."""

    title: str
    description: str
    status: Status = Status.PENDING
    tags: list[str] = field(default_factory=list)
    assignee: str | None = None

    def mark_complete(self) -> None:
        """Mark task as completed."""
        self.status = Status.COMPLETED

    def assign_to(self, person: str) -> None:
        """Assign task to a person.

        Args:
            person: Name of assignee
        """
        self.assignee = person
        if self.status == Status.PENDING:
            self.status = Status.IN_PROGRESS


@dataclass
class Project:
    """Project containing multiple tasks."""

    name: str
    tasks: list[Task] = field(default_factory=list)

    def add_task(self, task: Task) -> None:
        """Add a task to the project.

        Args:
            task: Task to add
        """
        self.tasks.append(task)

    def get_completed_tasks(self) -> list[Task]:
        """Get all completed tasks.

        Returns:
            List of completed tasks
        """
        return [task for task in self.tasks if task.status == Status.COMPLETED]

    def get_pending_tasks(self) -> list[Task]:
        """Get all pending tasks.

        Returns:
            List of pending tasks
        """
        return [task for task in self.tasks if task.status == Status.PENDING]
