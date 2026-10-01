# Issue tracker: GitHub

Issues and specs live in GitHub Issues on this repo. Use `gh`; it infers the repo from the clone.

- **Publish** (a spec, a ticket): `gh issue create`, multi-line body from a heredoc or `--body-file`.
- **Fetch a ticket**: `gh issue view <n> --comments`.
- **Labels**: the triage vocabulary is in `triage-labels.md`.
- **Triage covers issues only.** Pull requests here are the owner's own work and never enter the triage queue.

## Wayfinding

`/wayfinder` keeps a **map**: one issue labelled `wayfinder:map` holding Notes, Decisions so far, and Fog. Its tickets are child issues.

- **Child ticket**: a GitHub sub-issue of the map, labelled `wayfinder:<research|prototype|grilling|task>`. Without sub-issues, list the child in a task list in the map body and open the child body with `Part of #<map>`.
- **Blocking**: native issue dependencies. Add an edge with `gh api --method POST repos/<owner>/<repo>/issues/<child>/dependencies/blocked_by -F issue_id=<blocker-db-id>`. The blocker's database id comes from `gh api repos/<owner>/<repo>/issues/<n> --jq .id`; the `#number` and `node_id` are rejected. Without dependencies, open the child body with `Blocked by: #<n>, #<n>`. A ticket is unblocked once every blocker is closed.
- **Frontier**: the map's open children with zero open blockers (`issue_dependencies_summary.blocked_by`) and no assignee; the first in map order wins.
- **Claim**: `gh issue edit <n> --add-assignee @me`, as the session's first write.
- **Resolve**: comment the answer, close the issue, then append a one-line pointer (gist + link) to the map's Decisions so far.
