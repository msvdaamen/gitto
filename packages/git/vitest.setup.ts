// The tests create real repositories and commits. These variables override the repos' own
// `user.name`/`user.email`, so drop them to get the same authors on every machine.
for (const key of [
  "GIT_AUTHOR_NAME",
  "GIT_AUTHOR_EMAIL",
  "GIT_AUTHOR_DATE",
  "GIT_COMMITTER_NAME",
  "GIT_COMMITTER_EMAIL",
  "GIT_COMMITTER_DATE",
]) {
  delete process.env[key];
}
