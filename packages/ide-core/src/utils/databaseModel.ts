/**
 * The database a project uses.
 *
 * The rule that matters: when the driver changes, the connection string
 * follows it — but only if the user never chose one. A URL they did choose
 * is left alone, because they may know something the default does not.
 */

import type { DatabaseDriver } from '../types/project';

/**
 * The default connection string for each driver.
 *
 * MUST match `DatabaseDriver::default_url()` in
 * `crates/model/src/project.rs` — the Rust side is the source of truth, and
 * the two drifting apart would mean the IDE showing a URL the generated
 * backend does not use.
 *
 * SQLite's URL carries `?mode=rwc` because SQLite refuses to create a
 * missing file without it.
 */
export const DB_DRIVER_DEFAULT_URLS: Record<DatabaseDriver, string> = {
  sqlite: 'sqlite://app.db?mode=rwc',
  postgres: 'postgres://postgres:postgres@localhost:5432/app',
  mysql: 'mysql://user:pass@localhost:3306/app',
  // SeaORM 0.12 has no driver for either, so the generator refuses them.
  sql_server: '',
  oracle: '',
};

/** The database settings a project carries. */
export interface DatabaseSettings {
  driver: DatabaseDriver;
  url: string;
}

/**
 * Apply a driver change to a project's database settings.
 *
 * The connection string follows the driver when the user never touched it:
 * a project that arrived on SQLite holds `sqlite://app.db?mode=rwc`, and
 * switching to Postgres while keeping it would produce a backend that
 * cannot reach the database the user just chose.
 *
 * A URL the user DID choose survives the change.
 */
export function applyDatabaseDriver(
  database: DatabaseSettings,
  nextDriver: DatabaseDriver,
): DatabaseSettings {
  const current = database.url?.trim() ?? '';
  const untouched =
    current === '' || current === (DB_DRIVER_DEFAULT_URLS[database.driver] ?? '');

  return {
    ...database,
    driver: nextDriver,
    url: untouched ? DB_DRIVER_DEFAULT_URLS[nextDriver] ?? '' : database.url,
  };
}
