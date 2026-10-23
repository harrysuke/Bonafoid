/**
 * Tests for the database settings helper.
 *
 * The rule under test is the one that bit: a project arrives on SQLite
 * holding `sqlite://app.db?mode=rwc`, so changing the driver and keeping
 * that URL produces a backend that cannot reach the database the user just
 * chose.
 */

import { describe, expect, it } from 'vitest';
import { applyDatabaseDriver, DB_DRIVER_DEFAULT_URLS } from './databaseModel';
import type { DatabaseDriver } from '../types/project';

describe('applyDatabaseDriver', () => {
  it('carries the default URL when the driver changes', () => {
    const result = applyDatabaseDriver(
      { driver: 'sqlite', url: DB_DRIVER_DEFAULT_URLS.sqlite },
      'postgres',
    );

    expect(result.driver).toBe('postgres');
    expect(result.url).toBe(DB_DRIVER_DEFAULT_URLS.postgres);
  });

  it('fills in a URL when the project has none', () => {
    const result = applyDatabaseDriver({ driver: 'sqlite', url: '' }, 'mysql');

    expect(result.driver).toBe('mysql');
    expect(result.url).toBe(DB_DRIVER_DEFAULT_URLS.mysql);
  });

  it('keeps a URL the user chose', () => {
    const custom = 'postgres://user:pw@db.internal:5432/app';
    const result = applyDatabaseDriver(
      { driver: 'sqlite', url: custom },
      'postgres',
    );

    expect(result.url).toBe(custom);
  });

  it('leaves the driver alone when it does not change', () => {
    const result = applyDatabaseDriver(
      { driver: 'sqlite', url: DB_DRIVER_DEFAULT_URLS.sqlite },
      'sqlite',
    );

    expect(result.driver).toBe('sqlite');
    expect(result.url).toBe(DB_DRIVER_DEFAULT_URLS.sqlite);
  });

  it('has a default URL for every driver', () => {
    const drivers: DatabaseDriver[] = [
      'sqlite',
      'postgres',
      'mysql',
      'sql_server',
      'oracle',
    ];
    for (const driver of drivers) {
      expect(Object.keys(DB_DRIVER_DEFAULT_URLS)).toContain(driver);
    }
  });

  it('gives SQLite a URL that can create its own file', () => {
    // Without `mode=rwc`, SQLite refuses to create a missing file, so a
    // project that cannot create its own database cannot start.
    expect(DB_DRIVER_DEFAULT_URLS.sqlite).toContain('mode=rwc');
  });
});
