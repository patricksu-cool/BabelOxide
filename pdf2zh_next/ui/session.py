"""The state shape shared by GUI file and translation callbacks."""


def new_session_state() -> dict:
    """Return independent mutable containers for one GUI session."""
    return {
        "session_id": None,
        "current_task": None,
        "results": {},
        "file_order": [],
        "display_map": {},
        "parent_map": {},
        "uploaded_files": [],
    }
