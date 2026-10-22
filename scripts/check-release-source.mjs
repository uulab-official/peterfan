#!/usr/bin/env node
// Run from a trusted main checkout before executing a requested release tag.
// Inputs are environment variables so workflow input never becomes shell code.
import { execFileSync, spawnSync } from 'node:child_process';
import { appendFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

const numeric = '(?:0|[1-9][0-9]*)';
const identifier = '(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)';
const releaseTag = new RegExp(
  `^v${numeric}\\.${numeric}\\.${numeric}(?:-${identifier}(?:\\.${identifier})*)?(?:\\+[0-9A-Za-z-]+(?:\\.[0-9A-Za-z-]+)*)?$`,
);
const commitSha = /^[0-9a-f]{40}(?:[0-9a-f]{24})?$/;

export function resolveReleaseSource({ eventName, tag = '', workflowRef, ciSha, required = false, cwd = process.cwd() }) {
  if (!['push', 'pull_request', 'workflow_dispatch'].includes(eventName)) {
    throw new Error('Unsupported release-source event.');
  }
  // A normal CI run keeps GitHub's original push HEAD or PR merge commit. It
  // does not resolve user-selected refs or require that a PR is already merged.
  if (eventName !== 'workflow_dispatch' || tag === '') {
    if (required) throw new Error('An existing release tag is required.');
    if (!commitSha.test(ciSha ?? '') || ciSha.trim() !== ciSha) throw new Error('Invalid CI commit SHA.');
    return { release: 'false', tag: '', sha: ciSha, reviewed_sha: '' };
  }
  if (workflowRef !== 'refs/heads/main') {
    throw new Error('Tagged release dispatch must run from refs/heads/main.');
  }
  if (!releaseTag.test(tag) || tag.trim() !== tag) {
    throw new Error('Release tag must be canonical vMAJOR.MINOR.PATCH SemVer.');
  }
  const options = {
    cwd,
    encoding: 'utf8',
    env: { ...process.env, GIT_NO_REPLACE_OBJECTS: '1' },
    stdio: ['ignore', 'pipe', 'pipe'],
  };
  const resolve = (ref, description) => {
    let sha;
    try {
      sha = execFileSync('git', ['rev-parse', '--verify', `${ref}^{commit}`], options).trim();
    } catch {
      throw new Error(`Could not resolve ${description} to a commit.`);
    }
    if (!commitSha.test(sha)) throw new Error(`Invalid ${description} commit SHA.`);
    return sha;
  };
  const sha = resolve(`refs/tags/${tag}`, 'release tag');
  const reviewedSha = resolve('refs/remotes/origin/main', 'reviewed origin/main');
  const ancestor = spawnSync('git', ['merge-base', '--is-ancestor', sha, reviewedSha], options);
  if (ancestor.status !== 0) {
    throw new Error('Release tag commit must be an ancestor of reviewed origin/main.');
  }
  return { release: 'true', tag, sha, reviewed_sha: reviewedSha };
}

export function writeOutputs(result, outputFile) {
  if (!outputFile) throw new Error('GITHUB_OUTPUT is required.');
  appendFileSync(outputFile, Object.entries(result).map(([key, value]) => `${key}=${value}\n`).join(''));
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const result = resolveReleaseSource({
      eventName: process.env.RELEASE_EVENT_NAME,
      tag: process.env.RELEASE_TAG ?? '',
      workflowRef: process.env.RELEASE_WORKFLOW_REF,
      ciSha: process.env.RELEASE_CI_SHA,
      required: process.env.RELEASE_REQUIRED === 'true',
    });
    writeOutputs(result, process.env.GITHUB_OUTPUT);
    console.log(result.release === 'true'
      ? `Validated ${result.tag} at ${result.sha} against origin/main ${result.reviewed_sha}.`
      : `Preserving CI commit ${result.sha}; no release attachment requested.`);
  } catch (error) {
    console.error(`Release source rejected: ${error.message}`);
    process.exitCode = 1;
  }
}
