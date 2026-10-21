# Deviations from upstream

Every place this crate departs from upstream Python trafilatura at the pinned release, deliberate or
forced. The code carries `// deviation: D-n` at the spot. A row leaves when the difference is gone,
for example after upstream changed; ids are never reused.

| Id | Upstream behavior | This crate | Reason | Since |
| --- | --- | --- | --- | --- |
| D-1 | `fast=False` by default: readability and jusText run after the main extractor (`external.py`) and can replace its result | Fast mode only: no `fast` option, no readability or jusText, every `options.fast` branch takes the fast side | Keeps the first releases thin; a caller that needs a fallback runs one on a rejection. Porting `fast=False` is followup F-6 | first release |
