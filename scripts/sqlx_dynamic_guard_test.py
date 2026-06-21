#!/usr/bin/env python3
"""Fixture tests for the dynamic SQLx guard."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import sqlx_dynamic_guard as guard  # noqa: E402


class SqlxDynamicGuardTests(unittest.TestCase):
    def scan(self, source: str, path: str = "crates/example/src/lib.rs") -> list[guard.Finding]:
        return guard.scan_source(path, source)

    def assert_symbols(self, source: str, symbols: list[str]) -> None:
        self.assertEqual([finding.symbol for finding in self.scan(source)], symbols)

    def test_allowed_compile_checked_macros_are_ignored(self) -> None:
        findings = self.scan(
            """
            async fn checked(pool: &sqlx::PgPool) {
                let _ = sqlx::query!("SELECT 1").fetch_one(pool).await;
                let _ = sqlx::query_as!(Row, "SELECT 1 AS value").fetch_one(pool).await;
                let _ = sqlx::query_scalar!("SELECT 1").fetch_one(pool).await;
                let _ = sqlx::query_file!("queries/example.sql").fetch_one(pool).await;
            }
            """
        )
        self.assertEqual(findings, [])

    def test_forbidden_direct_calls_are_detected(self) -> None:
        self.assert_symbols(
            """
            async fn dynamic(pool: &sqlx::PgPool) {
                let _ = sqlx::query("SELECT 1").execute(pool).await;
                let _ = sqlx::query_as::<_, Row>("SELECT 1").fetch_all(pool).await;
                let _ = sqlx::query_scalar("SELECT 1").fetch_one(pool).await;
                let _ = sqlx::query_with("SELECT 1", args).execute(pool).await;
                let _ = sqlx::raw_sql("VACUUM").execute(pool).await;
            }
            """,
            ["query", "query_as", "query_scalar", "query_with", "raw_sql"],
        )

    def test_imported_aliases_and_renamed_uses_are_detected(self) -> None:
        self.assert_symbols(
            """
            use sqlx::{query as dyn_query, query_as, query_scalar as scalar, query_with as with_args};
            use sqlx::raw_sql as raw;

            async fn dynamic(pool: &sqlx::PgPool) {
                let _ = dyn_query("SELECT 1").execute(pool).await;
                let _ = query_as::<_, Row>("SELECT 1").fetch_one(pool).await;
                let _ = scalar("SELECT 1").fetch_one(pool).await;
                let _ = with_args("SELECT 1", args).execute(pool).await;
                let _ = raw("VACUUM").execute(pool).await;
            }
            """,
            ["query", "query_as", "query_scalar", "query_with", "raw_sql"],
        )

    def test_crate_aliases_and_query_builder_are_detected(self) -> None:
        self.assert_symbols(
            """
            use sqlx as sx;
            use sqlx::{Postgres, QueryBuilder as SqlBuilder};

            fn dynamic() {
                let _ = sx::query("SELECT 1");
                let mut direct = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT 1");
                let mut renamed = SqlBuilder::<Postgres>::new("SELECT 1");
                direct.push(" WHERE true");
                renamed.push(" WHERE true");
            }
            """,
            ["query", "QueryBuilder", "QueryBuilder"],
        )

    def test_tests_dir_and_cfg_test_modules_are_ignored(self) -> None:
        tests_dir_findings = self.scan(
            """
            async fn integration_test(pool: &sqlx::PgPool) {
                let _ = sqlx::query("SELECT 1").execute(pool).await;
            }
            """,
            path="crates/example/tests/integration.rs",
        )
        self.assertEqual(tests_dir_findings, [])

        cfg_findings = self.scan(
            """
            async fn production(pool: &sqlx::PgPool) {
                let _ = sqlx::query("SELECT 1").execute(pool).await;
            }

            #[cfg(test)]
            mod tests {
                async fn unit(pool: &sqlx::PgPool) {
                    let _ = sqlx::query("SELECT 2").execute(pool).await;
                }
            }
            """
        )
        self.assertEqual([finding.line for finding in cfg_findings], [3])

    def test_allowlist_accepts_precise_selector_and_rejects_new_use(self) -> None:
        findings = self.scan(
            """
            async fn first(pool: &sqlx::PgPool) {
                let _ = sqlx::query("SELECT 1").execute(pool).await;
            }
            async fn second(pool: &sqlx::PgPool) {
                let _ = sqlx::raw_sql("VACUUM").execute(pool).await;
            }
            """
        )
        allowlist = guard.Allowlist.from_mapping(
            {
                "exceptions": [
                    {
                        "path": findings[0].path,
                        "symbol": "query",
                        "selector": findings[0].selector,
                        "reason": "fixture exercises selector matching",
                        "reviewer": "LOW-570",
                        "expiration": "2026-12-31",
                        "removal_target": "replace fixture dynamic query",
                    }
                ]
            }
        )

        evaluation = guard.evaluate_allowlist(findings, allowlist)
        self.assertFalse(evaluation.ok)
        self.assertEqual([finding.symbol for finding in evaluation.unallowlisted], ["raw_sql"])
        self.assertEqual(evaluation.stale, ())

    def test_allowlist_reports_stale_lines(self) -> None:
        findings = self.scan(
            """
            async fn dynamic(pool: &sqlx::PgPool) {
                let _ = sqlx::query("SELECT 1").execute(pool).await;
            }
            """
        )
        allowlist = guard.Allowlist.from_mapping(
            {
                "exceptions": [
                    {
                        "path": findings[0].path,
                        "symbol": "query",
                        "line": findings[0].line + 10,
                        "reason": "fixture stale line",
                        "reviewer": "LOW-570",
                        "expiration": "2026-12-31",
                        "removal_target": "replace fixture dynamic query",
                    }
                ]
            }
        )

        evaluation = guard.evaluate_allowlist(findings, allowlist)
        self.assertFalse(evaluation.ok)
        self.assertEqual([finding.symbol for finding in evaluation.unallowlisted], ["query"])
        self.assertEqual(len(evaluation.stale), 1)


if __name__ == "__main__":
    unittest.main()
