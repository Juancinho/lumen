# Inventory service

Small web service that tracks stock levels for the shop.

## Installation

1. Install Node.js 20 and Rust 1.80 or newer.
2. Clone the repository and run `npm install` in `web/`.
3. Build the backend with `cargo build --release`.
4. Copy `.env.example` to `.env` and set `DATABASE_URL`.
5. Start everything with `docker compose up`.
