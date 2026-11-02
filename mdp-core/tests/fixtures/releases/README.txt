Release samples

Each folder holds the sample family as one catalog format version wrote it:
a backup (journey.mdp-backup.zip) and a zipped export (export.zip).
tests/release_samples.rs opens every one with the current build on every
change, so an update that would stop older Journeys, backups or exports
from opening fails CI before it reaches anyone.

1.4 was made by the main branch at sprint 12 (app 0.12.x, the tester builds).
1.5 was made by sprint 13.

When the format version goes up, save a sample for the new version:
    cargo run --example release_sample -- tests/fixtures/releases/<format>
Never change or delete a saved sample; add new ones.
