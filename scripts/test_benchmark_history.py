import copy
import unittest

from benchmark_history import compare


def report():
    return {
        "release_build": True,
        "options": {"samples": 10, "transactional": True, "history_auditing": False},
        "nats_server_version": "2.12.1",
        "runtime_workers": 2,
        "architecture": "x86_64",
        "os": "linux",
        "cases": [{
            "events_per_aggregate": 50,
            "total_events": 100,
            "source_payload_bytes": 1000,
            "encoded_snapshot_bytes": 100,
            "encoded_response_bytes": 50,
            "results": [{"path": "replay_sequential", "samples": 10,
                         "p50_us": 100, "p95_us": 200,
                         "published_messages_per_query": 104}],
        }],
    }


class ComparisonTests(unittest.TestCase):
    def test_computes_speedup_and_request_counts(self):
        before = report()
        after = copy.deepcopy(before)
        after["cases"][0]["results"][0].update(
            p50_us=10, p95_us=20, published_messages_per_query=10)
        result = compare(before, after)[0]
        self.assertEqual(result["p50_speedup"], 10)
        self.assertEqual(result["before_requests"], 104)
        self.assertEqual(result["after_requests"], 10)

    def test_rejects_different_workloads_and_environments(self):
        for field, value in (("options", {}), ("nats_server_version", "2.12.15"),
                             ("release_build", False), ("runtime_workers", 4)):
            with self.subTest(field=field):
                before, after = report(), report()
                after[field] = value
                with self.assertRaises(ValueError):
                    compare(before, after)

    def test_rejects_different_payloads_paths_or_samples(self):
        before = report()
        for field in ("total_events", "source_payload_bytes", "encoded_response_bytes"):
            with self.subTest(field=field):
                after = report()
                after["cases"][0][field] += 1
                with self.assertRaises(ValueError):
                    compare(before, after)
        for field, value in (("samples", 20), ("path", "another-path")):
            after = report()
            after["cases"][0]["results"][0][field] = value
            with self.assertRaises(ValueError):
                compare(before, after)

    def test_rejects_different_audit_policies(self):
        before, after = report(), report()
        after["options"]["history_auditing"] = True
        with self.assertRaises(ValueError):
            compare(before, after)


if __name__ == "__main__":
    unittest.main()
