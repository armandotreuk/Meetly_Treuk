-- `created_at` may be the recording start time. Keep persistence time separately
-- so temporal Chat questions can identify the latest saved/imported meeting.
ALTER TABLE meetings ADD COLUMN saved_at TEXT;

-- Historical persistence time was not recorded. `updated_at` is the closest
-- available application timestamp for existing meetings.
UPDATE meetings SET saved_at = updated_at WHERE saved_at IS NULL;

CREATE INDEX idx_meetings_saved_at ON meetings(saved_at DESC);
