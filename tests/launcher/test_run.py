"""Exercise launch decisions without Docker, credentials or network access."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class LauncherTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="agentway-launcher-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        shutil.copy2(ROOT / "run", self.root / "run")
        (self.root / ".agentway").mkdir()
        (self.root / "bin").mkdir()
        self.env = dict(os.environ, PATH=f"{self.root / 'bin'}:{os.environ['PATH']}",
                        CALLS=str(self.root / "calls"))
        self.stub("docker", '''#!/usr/bin/env bash
printf '%s\\n' "$*" >> "$CALLS"
if [[ "$*" == *'logs --no-color tunnel'* ]]; then
  echo https://temporary-example.trycloudflare.com
fi
''')
        self.stub("curl", '''#!/usr/bin/env bash
printf 'curl %s\\n' "$*" >> "$CALLS"
if [[ "${REJECT_PYTHON:-0}" == 1 && "$*" == *Python-urllib* ]]; then
  export RESPONSE_CODE=403 RESPONSE_BODY='error code: 1010'
fi
while (($#)); do
  if [[ "$1" == --output ]]; then
    printf '%s' "${RESPONSE_BODY:-AgentWay bearer token required}" > "$2"
    printf '%s' "${RESPONSE_CODE:-401}"
    exit 0
  fi
  shift
done
''')
        self.stub("sleep", "#!/usr/bin/env bash\nexit 0\n")

    def stub(self, name, content):
        path = self.root / "bin" / name
        path.write_text(content)
        path.chmod(0o700)

    def configure(self, hostname="dev.example.com"):
        (self.root / ".agentway/tunnel-hostname").write_text(hostname + "\n")
        (self.root / ".agentway/tunnel-token").write_text("fake-local-tunnel-credential")

    def run_launcher(self, *args):
        result = subprocess.run(["bash", str(self.root / "run"), "--no-open", *args],
                                cwd=self.root, env=self.env, capture_output=True, text=True, timeout=10)
        calls = (self.root / "calls").read_text()
        self.assertNotIn("fake-local-tunnel-credential", result.stdout + result.stderr + calls)
        return result, calls

    def test_named_tunnel_reused_with_or_without_share(self):
        self.configure()
        for args in [(), ("--share",)]:
            with self.subTest(args=args):
                (self.root / "calls").write_text("")
                result, calls = self.run_launcher(*args)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("-f compose.named-tunnel.yaml --profile share up", calls)
                self.assertNotIn("--force-recreate", calls)
                self.assertIn('"url":"https://dev.example.com"', calls)
                self.assertIn("stable address", result.stdout)

    def test_incomplete_or_invalid_configuration_does_not_start_stack(self):
        for hostname in [None, 'dev.example.com/anything', 'bad"hostname.example.com']:
            with self.subTest(hostname=hostname):
                self.configure(hostname or "dev.example.com")
                if hostname is None:
                    (self.root / ".agentway/tunnel-token").unlink()
                (self.root / "calls").write_text("")
                result, calls = self.run_launcher()
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn(" up ", calls)
                self.assertNotIn("curl", calls)

    def test_failed_reachability_preserves_saved_endpoint(self):
        self.configure()
        for code, body in [("503", "Unavailable"), ("401", "Unrelated proxy rejection")]:
            with self.subTest(code=code):
                self.env.update(RESPONSE_CODE=code, RESPONSE_BODY=body)
                (self.root / "calls").write_text("")
                result, calls = self.run_launcher()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("previous saved agent endpoint was preserved", result.stderr)
                self.assertNotIn("/api/publishing/bridge", calls)

    def test_browser_filter_block_is_not_reported_as_ready(self):
        self.configure()
        self.env["REJECT_PYTHON"] = "1"
        result, calls = self.run_launcher()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Python-urllib/3.12", calls)
        self.assertIn("browser/bot filtering", result.stderr)
        self.assertNotIn("/api/publishing/bridge", calls)

    def test_local_and_quick_tunnel_modes_remain_available(self):
        result, calls = self.run_launcher()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("compose.named-tunnel.yaml", calls)
        self.assertNotIn("curl", calls)
        (self.root / "calls").write_text("")
        result, calls = self.run_launcher("--share")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--force-recreate tunnel", calls)
        self.assertIn("temporary-example.trycloudflare.com", calls)


if __name__ == "__main__":
    unittest.main()
