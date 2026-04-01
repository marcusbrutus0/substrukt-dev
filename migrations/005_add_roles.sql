ALTER TABLE users ADD COLUMN role TEXT NOT NULL DEFAULT 'editor';
UPDATE users SET role = 'admin';

ALTER TABLE invitations ADD COLUMN role TEXT NOT NULL DEFAULT 'editor';
