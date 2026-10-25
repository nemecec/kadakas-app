# Security policy

## Supported versions

Kadakas has no stable release yet. Only the latest commit on `main` and the latest nightly build
get security fixes.

## Report a vulnerability

Do not open a public issue for a security problem.

Use a private report: open the **Security** tab of this repository and choose **Report a
vulnerability** (https://github.com/nemecec/kadakas-app/security/advisories/new). Only the
maintainers can read it.

Please include:

- the version or commit,
- the system (macOS, Windows or Linux),
- the steps or the file that shows the problem,
- what you think the effect is.

A file that crashes the app or uses a lot of memory counts as a security problem. Please send the
file.

## What happens next

The maintainers read the report, reproduce it, and write back. We fix the problem on `main`, and
we publish an advisory when the fix is out. We name the finder in the advisory if the finder wants.

## Scope

In scope: the app, the modeler, the launcher and updater, the command line tool, the JSON-RPC and
MCP servers, the file readers and writers (kad, DXF, IFC, STEP, OBJ, STL, glTF), and the CI
workflows of this repository.

Out of scope: problems in third-party libraries that we do not ship, and problems that need
a person with full access to the computer.
