-- Add customVocabulary column to settings table (global word/phrase list for transcription + summary)
ALTER TABLE settings ADD COLUMN customVocabulary TEXT DEFAULT '';