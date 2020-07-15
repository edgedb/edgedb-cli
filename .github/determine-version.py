#!/usr/bin/env python3
import os
import re
import sys


posint = r'(0|[1-9]\d*)'
shortform = re.compile(
    rf"""
    ^
    (?P<major>{posint})
    \.
    (?P<minor>{posint})
    (
        \.
        (?P<micro>{posint})
    )?
    (
        (?P<prekind>a|b|rc)
        (?P<preval>{posint})
    )?
    $
    """,
    re.X,
)
prerelease_kinds = {"a": "alpha", "b": "beta", "rc": "rc"}


def main():
    if len(sys.argv) != 2 or not sys.argv[1].strip():
        print(
            "error: branch name not passed as a sole argument."
            " On GHA pass it as part of `client_payload`.",
            file=sys.stderr,
        )
        sys.exit(1)

    branch = sys.argv[1].strip()
    if branch.startswith('v'):
        version = branch[1:]
    elif branch.startswith('release/'):
        version = branch[len('release/'):]
    else:
        print(f'::set-output name=version::{branch}')
        return

    m = shortform.match(version)
    if m:
        major = m.group('major')
        minor = m.group('minor')
        micro = m.group('micro') or '0'
        prekind = m.group('prekind')
        preval = m.group('preval')
        version = f"{major}.{minor}.{micro}"
        if prekind:
            longprekind = prerelease_kinds.get(prekind, "dev")
            version += f"-{longprekind}.{preval}"

    print(f'::set-output name=version::{version}')


if __name__ == "__main__":
    main()