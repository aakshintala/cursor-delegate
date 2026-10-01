# Domain docs

This repo is single-context: one `CONTEXT.md` (the glossary) and `docs/adr/` (decisions), both at the root. Both are created lazily by `/domain-modeling` once a term or decision is settled; until then they are absent, and that is expected.

Before exploring an area, read `CONTEXT.md` and the ADRs that touch it, when they exist.

- **Glossary terms**: name domain concepts (in issue titles, proposals, hypotheses, test names) with the term `CONTEXT.md` defines. A concept the glossary lacks is either invented language to reconsider or a gap to note for `/domain-modeling`.
- **ADR conflicts**: when your output contradicts an ADR, say so explicitly, in the form _Contradicts ADR-NNNN (<title>), but worth reopening because…_
