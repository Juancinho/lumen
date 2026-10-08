-- Speed up login lookups: users are found by email on every sign-in.
CREATE UNIQUE INDEX IF NOT EXISTS users_email_idx ON users (lower(email));

-- Orders are listed per customer, newest first.
CREATE INDEX IF NOT EXISTS orders_customer_created_idx ON orders (customer_id, created_at DESC);
