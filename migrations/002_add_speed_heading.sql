-- Optional motion data reported alongside a position. NULL means the client didn't report it.
ALTER TABLE user_coords ADD COLUMN speed REAL;   -- speed over ground in m/s
ALTER TABLE user_coords ADD COLUMN heading REAL; -- direction of travel, degrees clockwise from true north [0, 360)
