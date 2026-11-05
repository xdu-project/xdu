"""P3 gate: the release and CI workflows carry the extension steps in an order that cannot publish
an unchecked tarball. Structural only; the per-leg evidence is the pull request's own run.

Run through verify/p3.sh, which gives uv sandbox-safe cache directories.
"""

import sys
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[3]


def load(name):
    doc = yaml.safe_load((ROOT / ".github/workflows" / name).read_text())
    # PyYAML reads the bare key `on` as the boolean True.
    doc["on"] = doc.get("on", doc.get(True))
    return doc


def index_of(steps, needle, what):
    hits = [i for i, s in enumerate(steps) if needle in (s.get("run") or "") + str(s.get("uses") or "")]
    assert len(hits) == 1, f"{what}: expected exactly one step containing {needle!r}, found {hits}"
    return hits[0]


rel = load("release.yaml")

for job in ("build", "build-macos"):
    steps = rel["jobs"][job]["steps"]
    for s in steps:
        assert not s.get("continue-on-error"), f"{job}: a step may not continue on error: {s.get('name')}"
    b = index_of(steps, "--example extension-probe", f"{job} build")
    assert "--bins" in steps[b]["run"] and "--locked" in steps[b]["run"], f"{job}: one locked build of bins + probe"
    f = index_of(steps, "packaging/extensions/fetch.sh", f"{job} fetch")
    a = index_of(steps, "tar czvf", f"{job} assemble")
    assert "bin share lib" in steps[a]["run"], f"{job}: tarball must carry lib/"
    c = index_of(steps, "packaging/extensions/check.sh", f"{job} check")
    tarball = "xdu-${{ env.TAG }}-${{ matrix.target }}.tar.gz"
    assert "xdu-${TAG}-${{ matrix.target }}.tar.gz" in steps[c]["run"], f"{job}: check the tarball that is uploaded"
    assert "target/${{ matrix.target }}/release/examples/extension-probe" in steps[c]["run"], f"{job}: the probe comes from this leg's build"
    u = index_of(steps, "actions/upload-artifact", f"{job} upload")
    assert steps[u]["with"]["path"] == tarball, f"{job}: upload path drifted from the checked tarball"
    for i in (f, c):
        assert "if" not in steps[i], f"{job}: fetch/check may not be conditional"

    assert b < f < a < c < u, f"{job}: order must be build < fetch < assemble < check < upload, got {(b, f, a, c, u)}"
    if job == "build":
        g = next(i for i, s in enumerate(steps) if "GLIBC_2.28" in (s.get("run") or ""))
        assert g < f, "build: glibc floor runs before the fetch"

mac = {e["target"]: e["os"] for e in rel["jobs"]["build-macos"]["strategy"]["matrix"]["include"]}
assert mac == {"aarch64-apple-darwin": "macos-15", "x86_64-apple-darwin": "macos-15-intel"}, mac

pub = rel["jobs"]["publish"]
assert "pull_request" in str(pub.get("if", "")), "publish must be skipped on pull requests"
assert set(pub["needs"]) == {"build", "build-macos"}, pub["needs"]

pr = rel["on"].get("pull_request") or {}
paths = pr.get("paths") or []
for p in (".github/workflows/release.yaml", "packaging/extensions/**", "examples/**", "src/engine.rs",
          ".cargo/**", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml"):
    assert p in paths, f"release.yaml pull_request paths must include {p}"
assert "pull_request" in rel["env"]["TAG"], "TAG needs a pull-request fallback"
cancel = str(rel["concurrency"].get("cancel-in-progress"))
assert "pull_request" in cancel, f"cancel-in-progress must be PR-only, got {cancel!r}"

tests = load("test.yaml")
oses = tests["jobs"]["test"]["strategy"]["matrix"]["os"]
assert "macos-14" not in oses and "macos-15" in oses, oses

dock = load("docker.yaml")
dpaths = dock["on"]["pull_request"]["paths"]
for p in ("packaging/extensions/**", "examples/**"):
    assert p in dpaths, f"docker.yaml pull_request paths must include {p}"

for name in ("release.yaml", "test.yaml", "docker.yaml"):
    assert "macos-14" not in (ROOT / ".github/workflows" / name).read_text(), f"{name} still names macos-14"

print("P3-GATE-GREEN")
sys.exit(0)
