CREATE TABLE IF NOT EXISTS user_cords
(
    name      TEXT COLLATE NOCASE PRIMARY KEY, -- username is the primary key, simple.
    latitude  REAL NOT NULL,
    longitude REAL NOT NULL,
    timestamp TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
)