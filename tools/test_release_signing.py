import importlib.util
import os
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("sign_macos", Path(__file__).with_name("sign-macos.py"))
signer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(signer)


class Signing(unittest.TestCase):
    def test_process_failures_do_not_echo_secret_arguments(self):
        secret = "test-passphrase"
        with patch.object(signer.subprocess, "run", side_effect=subprocess.CalledProcessError(1, ["security", "-P", secret])):
            with self.assertRaises(RuntimeError) as error:
                signer.run("security", "-P", secret)
            self.assertNotIn(secret, str(error.exception))
        with patch.object(signer.subprocess, "check_output", side_effect=subprocess.CalledProcessError(1, ["xcrun", "--password", secret])):
            with self.assertRaises(RuntimeError) as error:
                signer.notarize(Path("archive.zip"), ["--password", secret])
            self.assertNotIn(secret, str(error.exception))

    def test_notarization_requires_accepted_status(self):
        with patch.object(signer.subprocess, "check_output", return_value='{"status":"Invalid","id":"submission-id"}'):
            with self.assertRaisesRegex(RuntimeError, "notarization failed"):
                signer.notarize(Path("archive.zip"), [])
        with patch.object(signer.subprocess, "check_output", return_value='{"status":"Accepted","id":"submission-id"}'):
            signer.notarize(Path("archive.zip"), [])

    def test_missing_credentials_fail_before_running_signing_tools(self):
        with patch.dict(os.environ, {}, clear=True), patch.object(signer, "run") as run:
            with self.assertRaisesRegex(ValueError, "protected signing secrets"):
                signer.sign(Path("payload"), Path("output"), "1.0.0")
            run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
