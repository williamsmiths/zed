CREATE TABLE council_sessions (
    id SERIAL PRIMARY KEY,
    project_id INTEGER NOT NULL,
    supervisor_participant_id INTEGER,
    phase TEXT NOT NULL DEFAULT 'frame',
    round INTEGER NOT NULL DEFAULT 0,
    authority TEXT NOT NULL DEFAULT 'human_final',
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL DEFAULT now(),
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL DEFAULT now()
);

CREATE TABLE council_participants (
    id SERIAL PRIMARY KEY,
    session_id INTEGER NOT NULL REFERENCES council_sessions (id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    user_id INTEGER,
    agent_label TEXT NOT NULL DEFAULT '',
    model TEXT NOT NULL DEFAULT '',
    tool TEXT NOT NULL DEFAULT '',
    replica_id INTEGER NOT NULL,
    active BOOLEAN NOT NULL DEFAULT true,
    joined_at TIMESTAMP WITHOUT TIME ZONE NOT NULL DEFAULT now(),
    left_at TIMESTAMP WITHOUT TIME ZONE
);
CREATE INDEX index_council_participants_on_session_id ON council_participants (session_id);

CREATE TABLE council_entries (
    id SERIAL PRIMARY KEY,
    session_id INTEGER NOT NULL REFERENCES council_sessions (id) ON DELETE CASCADE,
    author_participant_id INTEGER NOT NULL REFERENCES council_participants (id),
    lamport_value INTEGER NOT NULL,
    lamport_replica_id INTEGER NOT NULL,
    kind TEXT NOT NULL,
    body TEXT NOT NULL,
    refs TEXT NOT NULL DEFAULT '[]',
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL DEFAULT now()
);
CREATE INDEX index_council_entries_on_session_id ON council_entries (session_id);

CREATE TABLE work_items (
    id SERIAL PRIMARY KEY,
    project_id INTEGER NOT NULL,
    session_id INTEGER NOT NULL REFERENCES council_sessions (id) ON DELETE CASCADE,
    source_entry_id INTEGER REFERENCES council_entries (id),
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'todo',
    assignee_participant_id INTEGER REFERENCES council_participants (id),
    parent_id INTEGER,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL DEFAULT now(),
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL DEFAULT now()
);
CREATE INDEX index_work_items_on_session_id ON work_items (session_id);
