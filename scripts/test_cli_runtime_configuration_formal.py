"""Keep runtime saved-configuration checks in the complete generic formal matrix."""

import json
import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MODEL = "agent-api-host/admission/RuntimeConfiguration"
POSITIVE = MODEL + "Positive.cfg"
CONTROLS = {
    "runtime-configuration-legacy-snapshot": (
        "LegacySnapshotNegative", "snapshot", "InvalidSavedConfigurationNoEffects", "invariant"),
    "runtime-configuration-identity-guard": (
        "IdentityNegative", "identity", "AdmittedIdentityIsCurrentLocal", "invariant"),
    "runtime-configuration-start-changed-witness": (
        "StartChangedWitness", "none", "NoStartChangedConfigurationRejection", "invariant"),
    "runtime-configuration-restart-changed-witness": (
        "RestartChangedWitness", "none", "NoRestartChangedConfigurationRejection", "invariant"),
    "runtime-configuration-local-changed-witness": (
        "LocalChangedWitness", "none", "NoChangedLocalIdentityAdmission", "invariant"),
    "runtime-configuration-validation-lease": (
        "ValidationLeaseNegative", "validation_lease", "LifecycleHeldDuringValidation", "invariant"),
    "runtime-configuration-restart-lease": (
        "RestartLeaseNegative", "restart_lease", "LifecycleHeldDuringValidation", "invariant"),
    "runtime-configuration-terminal-release": (
        "TerminalReleaseNegative", "terminal_release", "TerminalGuardsReleased", "invariant"),
    "runtime-configuration-actor-progress": (
        "ActorProgressNegative", "actor_settlement", "AdmissionCompletes", "property"),
    "runtime-configuration-restart-progress": (
        "RestartProgressNegative", "restart_settlement", "AdmissionCompletes", "property"),
    "runtime-configuration-restart-pending-witness": (
        "RestartPendingWitness", "none", "NoPostActorRestartPending", "invariant"),
}
PROPERTIES = {
    "TypeOK", "InvalidSavedConfigurationNoEffects", "InvalidSavedConfigurationRejected",
    "AdmittedIdentityIsCurrentLocal", "LifecycleHeldDuringValidation",
    "TerminalGuardsReleased", "AdmissionCompletes",
}
ACTIONS = {
    "ValidateInitialConfiguration", "AcquireLifecycle", "ChangeConfigurationWhileWaiting", "ReleaseExternalLifecycle",
    "ValidateCurrentConfiguration", "BeginRestartStop", "TransferStartAdmission",
    "AcquireActorAsr", "SettleActorStop", "ReleaseActorAsr",
    "SettleRestartStop", "TransferRestartAdmission",
}


class RuntimeConfigurationRegistrationTests(unittest.TestCase):
    def setUp(self):
        self.manifest = json.loads((ROOT / "formal/verification.json").read_text(encoding="utf-8"))

    def test_runtime_configuration_extends_the_existing_host_contract(self):
        contract = self.manifest["contracts"]["agent-host"]
        self.assertIn(MODEL, contract["models"])
        self.assertEqual(set(contract["models"][MODEL]), PROPERTIES | ACTIONS)
        self.assertEqual(set(self.manifest["actions"][MODEL]), ACTIONS)
        self.assertIn("agent-host", self.manifest["protocols"][
            "agent.authenticated-loopback-broker-and-controller"])
        for action in ["start", "restart"]:
            self.assertIn(
                "src-tauri/src/agent_api/tests.rs#async fn "
                f"controller_lifecycle_admission_engine_{action}_rechecks_saved_configuration()",
                contract["evidence"],
            )

    def test_generic_matrix_executes_the_positive_and_all_named_controls(self):
        self.assertIn(MODEL, self.manifest["runs"])
        self.assertEqual(self.manifest["runs"][MODEL], [POSITIVE])
        rows = [row for row in self.manifest["negative"] if row["model"] == MODEL]
        self.assertEqual(len(rows), len(CONTROLS))
        self.assertEqual({row["name"] for row in rows}, set(CONTROLS))
        for row in rows:
            suffix, wrong, target, kind = CONTROLS[row["name"]]
            self.assertEqual(row["config"], MODEL + suffix + ".cfg")
            self.assertEqual(row[kind], target)
            self.assertNotIn("property" if kind == "invariant" else "invariant", row)
            config = (ROOT / "formal" / row["config"]).read_text(encoding="utf-8")
            self.assertRegex(config, rf'(?m)^CONSTANT Wrong = "{wrong}"$')
            if kind == "property":
                self.assertEqual(re.findall(r"(?m)^PROPERTY (\w+)$", config), [target])
                self.assertNotRegex(config, r"(?m)^INVARIANT\b")
            else:
                self.assertRegex(config, rf"(?m)^INVARIANT {target}$")
            self.assertRegex(config, r"(?m)^CHECK_DEADLOCK TRUE$")
            self.assertNotRegex(config, r"(?m)^\s*(?:CONSTRAINT|ACTION_CONSTRAINT|SYMMETRY|VIEW)\b")

    def test_positive_activates_every_bound_property_without_state_restrictions(self):
        self.assertIn(MODEL, self.manifest["runs"])
        config = (ROOT / "formal" / POSITIVE).read_text(encoding="utf-8")
        checked = set(re.findall(r"(?m)^(?:INVARIANT|PROPERTY) (\w+)$", config))
        self.assertEqual(checked, PROPERTIES)
        self.assertRegex(config, r'(?m)^CONSTANT Wrong = "none"$')
        self.assertRegex(config, r"(?m)^CHECK_DEADLOCK TRUE$")
        self.assertNotRegex(config, r"(?m)^\s*(?:CONSTRAINT|ACTION_CONSTRAINT|SYMMETRY|VIEW)\b")

    def test_separately_accepted_unicode_matrix_remains_fully_registered(self):
        model = "cli/UnicodeSerialization"
        self.assertIn(model, self.manifest["runs"])
        self.assertEqual(self.manifest["runs"][model], [model + ".cfg"])
        self.assertEqual(set(self.manifest["contracts"]["agent-cli"]["models"][model]),
                         {"DecodedStringsPreserved", "Encode", "Pipe", "Decode"})
        self.assertEqual(set(self.manifest["actions"][model]), {"Encode", "Pipe", "Decode"})
        expected = {
            "cli-unicode-lossy-negative": ("LossyNegative", "DecodedStringsPreserved"),
            "cli-unicode-scalar-witness": ("ScalarWitness", "WitnessNotReached"),
            "cli-unicode-high-witness": ("HighWitness", "WitnessNotReached"),
            "cli-unicode-low-witness": ("LowWitness", "WitnessNotReached"),
            "cli-unicode-mixed-witness": ("MixedWitness", "WitnessNotReached"),
        }
        rows = [row for row in self.manifest["negative"] if row["model"] == model]
        self.assertEqual(len(rows), len(expected))
        self.assertEqual({row["name"] for row in rows}, set(expected))
        for row in rows:
            suffix, target = expected[row["name"]]
            self.assertEqual(row["config"], model + suffix + ".cfg")
            self.assertEqual(row["invariant"], target)
            self.assertNotIn("property", row)


if __name__ == "__main__":
    unittest.main()
