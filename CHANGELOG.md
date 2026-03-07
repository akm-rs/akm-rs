## alpha.5

Breaking fix: akm update was broken due to a misconfigured URL. To fix:
  akm config update.url https://api.github.com/repos/akm-rs/akm-rs/releases/latest
  Then akm update works normally. Alternatively, re-run the install script.
  Future installs are unaffected — this release auto-migrates the bad URL on startup.

## alpha.4

Fix : akm sync no longer resets the core: values for skills

## alpha.3 

Fix : akm instructions no longer replaces existing global instructions with empty new ones.
