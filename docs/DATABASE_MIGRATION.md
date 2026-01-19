# Database Migration Guide

## Overview

This document describes the process for migrating the user database from SQLite to PostgreSQL.

## Prerequisites

- PostgreSQL 15+
- Rust sqlx CLI
- Backup of existing SQLite database

## Migration Steps

1. Export data from SQLite
2. Create PostgreSQL schema
3. Import data with transformations
4. Verify data integrity
5. Update connection strings

## Rollback Plan

Keep SQLite database intact for 30 days post-migration.
