# Maintenance evidence

- The numbered reports in this directory preserve the first maintenance round.
- [Round 2 final report](round2/10_final_report.md) records the previous stable
  baseline, PDF comparisons, Windows packaging and known limitations.
- [Round 3 final report](round3/08_final_report.md) includes the final regression,
  architecture, cleanup, security limitations and stage commits.
- [Round 4 final report](round4/07_final_report.md) records authorized SiliconFlowFree
  tests, original-version PDF comparisons, Gradio resource diagnostics, and the
  Windows release candidate. Standard tests pass; strict resource checks, Pillow
  findings and independent clean-Windows acceptance remain release limitations.
- [Round 3 baseline](round3/01_baseline.md), [network security](round3/02_network_security.md),
  [Gradio compatibility](round3/03_compatibility.md), [remaining repairs](round3/04_repairs.md)
  and [deletion manifest](round3/05_deletion_manifest.json) cover the current work.

Raw executable bundles, local settings, caches, full command logs and translated
test PDFs stay in the ignored `.test-output/` directory. Reports contain concise
results and reproducible metrics; they are not a substitute for current tests.
No historical report is silently rewritten to claim that an earlier failure passed.
