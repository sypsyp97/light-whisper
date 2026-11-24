"""Local pre-commit and pre-push checks for Light Whisper contributors."""

import os
import re
import subprocess
import sys
from urllib.parse import urlsplit


class Blocked(Exception):
    """A diagnostic safe to print without repository data."""


SECRET = re.compile(
    rb"gh[pousr]_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{22}"
    rb"|hf_[A-Za-z0-9]{30}"
    rb"|-----BEGIN (?:[A-Z0-9 ]{1,40} )?PRIVATE KEY(?: BLOCK)?-----"
)
API_KEY = re.compile(rb"(?<![A-Za-z0-9])sk-[A-Za-z0-9_-]{20}")
# Limit literal authority length so a huge userinfo field cannot straddle the
# scanner's windows unchecked. Valid DNS names and ports fit well within this.
AUTH_CHAR = rb"[^/\s?#@<>{}\"\\]"
CREDENTIAL_URL = re.compile(rb"https?://(?:" + AUTH_CHAR + rb"{0,1024}@|" + AUTH_CHAR + rb"{1025})")
NOREPLY = re.compile(rb"[^<>\s@]+@users\.noreply\.github\.com", re.I)
EMAIL = re.compile(rb"[\w.+%'-]+@[\w.-]+\.[A-Za-z]{2,}\b")
OID = re.compile(rb"(?:[0-9a-f]{40}|[0-9a-f]{64})")


def markers_for(value):
    return {value.encode(codec).replace(b"\0", b"").lower()
            for codec in ("utf-8", "utf-16-le", "utf-16-be")}


def home_paths():
    paths = set()
    for key in ("HOME", "USERPROFILE"):
        value = os.environ.get(key, "").replace("\\", "/").rstrip("/")
        if not value or value.endswith(":"):
            continue
        for spelling in (value, value.replace("/", "\\"), value.replace("/", "\\\\")):
            paths.update(markers_for(spelling))
    return paths


def repository_markers():
    markers = home_paths()
    try:
        emails = git("config", "--local", "--null", "--get-all", "privacy.privateEmail")
    except subprocess.CalledProcessError as error:
        if error.returncode != 1:  # Git returns 1 for an absent key.
            raise
    else:
        for email in emails.split(b"\0"):
            if email:
                markers.update(markers_for(email.decode()))
    return markers


def check_content(data, paths):
    # Dropping NULs also exposes ASCII secrets embedded in UTF-16LE/BE binaries.
    plain = data.replace(b"\0", b"")
    if SECRET.search(plain) or (b"sk-" in plain and API_KEY.search(plain)):
        raise Blocked("possible access token or private key")
    lower = plain.lower()
    if b"://" in lower and CREDENTIAL_URL.search(lower):
        raise Blocked("possible credentials in an HTTP URL")
    if any(path in lower for path in paths):
        raise Blocked("local profile path or configured private email")


def check_identity(line):
    email = re.search(rb"<([^<>]*)> \d+ [+-]\d{4}$", line)
    if not email or not NOREPLY.fullmatch(email[1]):
        raise Blocked("use a GitHub noreply email for Git identities")


def git(*args, data=b""):
    return subprocess.run(
        ["git", "--no-replace-objects", *args], input=data,
        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, check=True,
    ).stdout


def oid_list(items):
    return b"".join(item + b"\n" for item in items)


def check_objects(oids, paths):
    if not oids:
        return
    # Request objects one at a time: neither a full pack nor large blobs need
    # to fit in memory. Keep enough overlap for every finite-length matcher.
    overlap = 2 * max(1040, max(map(len, paths), default=0))
    with subprocess.Popen(
        ["git", "--no-replace-objects", "cat-file", "--batch"],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
    ) as process:
        for oid in oids:
            if not OID.fullmatch(oid):
                raise ValueError
            process.stdin.write(oid + b"\n")
            process.stdin.flush()
            received, kind, size = process.stdout.readline().split()
            if received != oid or kind not in (b"blob", b"tree", b"commit", b"tag"):
                raise ValueError
            remaining = int(size)
            if kind in (b"commit", b"tag"):
                metadata = process.stdout.read(remaining)
                if len(metadata) != remaining:
                    raise ValueError
                check_content(metadata, paths)
                header = metadata.partition(b"\n\n")[0].split(b"\n")
                for line in header:
                    if line.startswith((b"author ", b"committer ", b"tagger ")):
                        check_identity(line)
                if any(not NOREPLY.fullmatch(email) for email in EMAIL.findall(metadata)):
                    raise Blocked("private email in outgoing commit or tag")
            else:
                tail = b""
                while remaining:
                    chunk = process.stdout.read(min(remaining, 1024 * 1024))
                    if not chunk:
                        raise ValueError
                    window = tail + chunk
                    check_content(window, paths)
                    tail = window[-overlap:]
                    remaining -= len(chunk)
            if process.stdout.read(1) != b"\n":
                raise ValueError
        process.stdin.close()
        if process.wait():
            raise ValueError


def pre_commit(paths):
    for role in ("AUTHOR", "COMMITTER"):
        identity = git("var", f"GIT_{role}_IDENT").rstrip(b"\n")
        check_identity(identity)
        check_content(identity, paths)
    changes = git("diff", "--cached", "--raw", "--no-abbrev", "--no-renames",
                  "--no-ext-diff", "--ignore-submodules=none", "-z", "--")
    fields = iter(changes.split(b"\0")[:-1])
    blobs = set()
    for record in fields:
        _, mode, _, oid, _ = record.split()
        name = next(fields)
        if mode != b"000000":
            check_content(name, paths)
            if mode != b"160000":  # Gitlinks identify commits in another repository.
                blobs.add(oid)
    check_objects(blobs, paths)


def pre_push(url, paths):
    if url.lower().startswith(("http://", "https://")) and "@" in urlsplit(url).netloc:
        raise Blocked("remove credentials from the remote URL")
    incoming = set()
    published = set()
    for line in sys.stdin.buffer:
        _, local, target, old = line.split()
        if not OID.fullmatch(local) or not OID.fullmatch(old):
            raise ValueError
        if local.strip(b"0"):
            check_content(target, paths)
            incoming.add(local)
        if old.strip(b"0"):
            published.add(old)
    if not incoming:
        return

    # A new ref supplies no old tip. Use the actual push destination; local
    # remote-tracking refs can be stale or point at never-published commits.
    advertised = {line.split(b"\t")[0] for line in git("ls-remote", "--refs", "--", url).splitlines()}
    if not all(OID.fullmatch(oid) for oid in advertised):
        raise ValueError
    known = git("cat-file", "--batch-check", data=oid_list(advertised | published))
    excluded = [b"^" + line.split()[0] for line in known.splitlines()
                if not line.endswith(b" missing")]
    objects = git("rev-list", "--objects", "--no-object-names", "--stdin",
                  data=oid_list([*incoming, *excluded]))
    check_objects(objects.splitlines(), paths)


def main():
    os.environ["GIT_TERMINAL_PROMPT"] = "0"
    os.environ["GIT_NO_LAZY_FETCH"] = "1"
    try:
        paths = repository_markers()
        if sys.argv[1:] == ["pre-commit"]:
            pre_commit(paths)
        elif len(sys.argv) == 4 and sys.argv[1] == "pre-push":
            pre_push(sys.argv[3], paths)
        else:
            raise ValueError
    except Blocked as error:
        print(f"Git privacy check blocked this operation: {error}.", file=sys.stderr)
        return 1
    except (OSError, ValueError, StopIteration, subprocess.CalledProcessError):
        print("Git privacy check could not verify the operation; blocked.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
