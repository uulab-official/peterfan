#!/usr/bin/env python3
"""Freeze a read-only installed-PeterFan observation in a temporary launchd job.

Requires a previously verified launch.json baseline. Does not install/update an
app, alter fan control, change settings, or create a persistent login item.
Calibration runs must explicitly use --calibration and never replace six-hour
acceptance. The caller updates its active-run pointer only after valid samples.
"""
import argparse
from datetime import datetime, timedelta, timezone
import hashlib
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
from zoneinfo import ZoneInfo


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def identity(pid):
    return subprocess.check_output(['/bin/ps', '-p', str(pid), '-o', 'lstart='],
                                   text=True, env={**os.environ, 'LC_ALL': 'C'}).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', type=Path, required=True)
    parser.add_argument('--seconds', type=int, default=21600)
    parser.add_argument('--interval', type=int, default=10)
    parser.add_argument('--calibration', action='store_true')
    args = parser.parse_args()
    if sys.platform != 'darwin':
        parser.error('launchd observation requires macOS')
    if not 30 <= args.seconds <= 604800 or not 1 <= args.interval <= min(3600, args.seconds):
        parser.error('duration/interval outside observer limits')
    if not args.calibration and (args.seconds < 21600 or args.interval != 10):
        parser.error('production observation requires >=21600 seconds and ten-second samples')
    os.umask(0o077)
    baseline = json.loads(args.baseline.read_text())
    for field, path in [('app_sha256', baseline['app_executable']),
                        ('daemon_sha256', '/usr/local/bin/peterfand'),
                        ('config_sha256', baseline['config_path'])]:
        if sha(path) != baseline[field]:
            raise RuntimeError(field + ' differs from verified baseline; refusing observation')
    for kind in ('app', 'daemon'):
        if identity(baseline[kind + '_pid']) != baseline[kind + '_started']:
            raise RuntimeError(kind + ' identity changed; inspect before creating a new baseline')
    root = Path.home() / 'Library/Application Support/PeterFan QA'
    root.mkdir(mode=0o700, exist_ok=True)
    if root.is_symlink() or root.stat().st_uid != os.getuid():
        raise RuntimeError('private observation directory is not owned by this user')
    os.chmod(root, 0o700)
    if shutil.disk_usage(root).free < 1024**3:
        raise RuntimeError('at least 1 GiB free is required before starting')
    now = datetime.now(timezone.utc)
    stamp = now.strftime('%Y%m%dT%H%M%S%fZ')
    mode = 'calibration' if args.calibration else 'production-six-hour'
    run = root / (mode + '-' + stamp)
    run.mkdir(mode=0o700)
    scripts = Path(__file__).resolve().parent
    for source, name in [('macos-soak.py', 'observer-source.py'),
                         ('macos-production-soak.py', 'supervisor.py'),
                         ('start-installed-soak.py', 'launcher-source.py')]:
        shutil.copyfile(scripts / source, run / name)
    config = Path(baseline['config_path'])
    shutil.copyfile(config.with_name('update-check-history.json'), run / 'update-history-baseline.json')
    shutil.copyfile(args.baseline, run / 'previous-verified-baseline.json')
    label = 'kr.co.uulab.peterfan.qa-soak.' + stamp.lower()
    plan = {**baseline, 'daemon_version': baseline.get('daemon_version', baseline['version']),
            'started_at': now.isoformat(), 'started_unix': int(now.timestamp()),
            'expected_end_at': (now + timedelta(seconds=args.seconds)).isoformat(),
            'expected_end_kst': (now + timedelta(seconds=args.seconds)).astimezone(ZoneInfo('Asia/Seoul')).isoformat(),
            'requested_seconds': args.seconds, 'sample_interval_seconds': args.interval,
            'require_scheduled_update': not args.calibration,
            'scope': 'calibration only; not six-hour acceptance' if args.calibration else 'same installed app and daemon on one Mac; no forced sleep, heat or app interruption',
            'job_label': label, 'run_directory': str(run),
            'observer_sha256': sha(run / 'observer-source.py'),
            'supervisor_sha256': sha(run / 'supervisor.py'),
            'launcher_sha256': sha(run / 'launcher-source.py'),
            'observer_checkout_commit': subprocess.check_output(['git', '-C', str(scripts.parent), 'rev-parse', 'HEAD'], text=True).strip(),
            'menubar_log_path': str(Path.home() / 'Library/Logs/PeterFan/menubar.log')}
    plan['menubar_log_offset'] = Path(plan['menubar_log_path']).stat().st_size
    (run / 'launch.json').write_text(json.dumps(plan, indent=2) + '\n')
    job = {'Label': label, 'ProgramArguments': [sys.executable, str(run / 'supervisor.py')],
           'WorkingDirectory': str(run), 'RunAtLoad': True, 'KeepAlive': False,
           'StandardOutPath': str(run / 'supervisor.log'),
           'StandardErrorPath': str(run / 'supervisor-error.log'), 'Umask': 0o077}
    plist = run / 'job.plist'
    plist.write_bytes(plistlib.dumps(job))
    # Plist stays outside LaunchAgents: no automatic restart at next login and
    # no KeepAlive retry that could silently concatenate observations.
    subprocess.run(['/bin/launchctl', 'bootstrap', 'gui/' + str(os.getuid()), str(plist)], check=True)
    print(json.dumps({'run_directory': str(run), 'job_label': label,
                      'expected_end_kst': plan['expected_end_kst'],
                      'scope': plan['scope']}, indent=2))


if __name__ == '__main__':
    main()
