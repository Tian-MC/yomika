import unittest
from rpm_config import rpm_config


class RpmVersionTests(unittest.TestCase):
    def test_prerelease_has_valid_separate_rpm_fields(self):
        config = rpm_config("0.1.1-alpha.2")
        self.assertEqual(config["version"], "0.1.1")
        self.assertEqual(config["bundle"]["linux"]["rpm"]["release"], "0.alpha.2")

    def test_final_release_uses_default_release_number(self):
        config = rpm_config("0.1.1")
        self.assertEqual(config["version"], "0.1.1")
        self.assertEqual(config["bundle"]["linux"]["rpm"]["release"], "1")

    def test_hyphens_and_build_metadata_are_safe_for_rpm(self):
        config = rpm_config("1.2.3-rc-test.4+build.5")
        self.assertEqual(config["version"], "1.2.3")
        self.assertEqual(config["bundle"]["linux"]["rpm"]["release"], "0.rc.test.4")

    def test_invalid_version_is_rejected(self):
        for version in ["", "v1.2.3", "1.2", "1.2.3/bad", "01.2.3"]:
            with self.subTest(version=version), self.assertRaises(ValueError):
                rpm_config(version)


if __name__ == "__main__":
    unittest.main()
