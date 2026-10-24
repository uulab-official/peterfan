#!/usr/bin/env python3
"""Observe pinned installed app/helper without modifying either.

Run only from a frozen private run directory containing launch.json and
observer-source.py. start-installed-soak.py registers this as a temporary
per-user launchd job. A missing terminal receipt is never a passing run.
"""
import csv
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import shutil
import signal
import socket
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parent
COUNTERS = ('sensor_failure_count', 'fan_write_failure_count', 'fan_readback_failure_count')
LIMITS = {'average_cpu_percent': 8, 'peak_cpu_percent': 35, 'peak_rss_kb': 262144,
          'rss_growth_kb': 65536, 'thread_growth': 16}
DISK_FLOOR = 512 * 1024 * 1024


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def save(name, value):
    temporary = ROOT / (name + '.tmp')
    with temporary.open('w') as stream:
        stream.write(json.dumps(value, indent=2) + '\n')
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, ROOT / name)


def ipc(command):
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(3)
        connection.connect('/var/run/peterfand.sock')
        connection.sendall((command + '\n').encode())
        with connection.makefile() as stream:
            return stream.readline(262144).strip()


def healthy_cache(version):
    reply = ipc('temps')
    if not reply.startswith('ok '):
        raise RuntimeError('missing helper temperature response')
    cache = json.loads(reply[3:])
    health = cache['control_health']
    if (cache['daemon_version'] != version or cache['mode'] != 'auto'
            or cache['fan_targets'] or cache['fan_overrides']
            or cache['applied_control_revision'] != cache['control_revision']
            or cache['temps_stale'] or health['failsafe_active']
            or health['last_error'] is not None or any(health[k] != 0 for k in COUNTERS)):
        raise RuntimeError('helper health/automatic control acceptance failed: ' + json.dumps(cache))
    return cache


def app_observer_environment(plan):
    return {**os.environ, 'PETERFAN_SOAK_PID': str(plan['app_pid']),
            'PETERFAN_MENUBAR_BIN': plan['app_executable'],
            'PETERFAN_SOAK_SECONDS': str(plan['requested_seconds']),
            'PETERFAN_SOAK_SAMPLE_SECONDS': str(plan['sample_interval_seconds']),
            'PETERFAN_SOAK_LOG_DIR': str(ROOT / 'observer'),
            'PETERFAN_SOAK_MAX_AVG_CPU': str(LIMITS['average_cpu_percent']),
            'PETERFAN_SOAK_MAX_PEAK_CPU': str(LIMITS['peak_cpu_percent']),
            'PETERFAN_SOAK_MAX_RSS_KB': str(LIMITS['peak_rss_kb']),
            'PETERFAN_SOAK_MAX_RSS_GROWTH_KB': str(LIMITS['rss_growth_kb']),
            'PETERFAN_SOAK_MAX_THREAD_GROWTH': str(LIMITS['thread_growth'])}


def validate_app_summary(summary, duration):
    if (not summary['passed'] or summary['requested_seconds'] != duration
            or not math.isfinite(summary['measured_seconds'])
            or summary['measured_seconds'] < duration or not summary['config_preserved']):
        raise RuntimeError('app terminal acceptance failed')
    if summary['thresholds'] != LIMITS:
        raise RuntimeError('app observer acceptance limits differ from production limits')
    for metric, limit in LIMITS.items():
        value = summary['metrics'][metric]
        if not math.isfinite(value) or value > limit:
            raise RuntimeError('app threshold exceeded or invalid: ' + metric)


def main():
    os.umask(0o077)
    plan = json.loads((ROOT / 'launch.json').read_text())
    spec = importlib.util.spec_from_file_location('frozen_soak', ROOT / 'observer-source.py')
    observer = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = observer
    spec.loader.exec_module(observer)
    duration, interval = plan['requested_seconds'], plan['sample_interval_seconds']
    result = {'passed': False, 'requested_seconds': duration, 'samples': 0, 'errors': [],
              'scope': plan['scope'], 'started_at': observer.utc_now(),
              'daemon_thresholds': LIMITS, 'job_label': plan['job_label']}
    child = assertion = app_log = reservation = None
    previous_handlers = {}

    def interrupted(number, _frame):
        raise RuntimeError('observer interrupted by ' + signal.Signals(number).name)

    try:
        reservation = observer.reserve_result_space(ROOT)
        for number in (signal.SIGTERM, signal.SIGINT):
            previous_handlers[number] = signal.signal(number, interrupted)
        if shutil.disk_usage(ROOT).free < 2 * DISK_FLOOR:
            raise RuntimeError('less than 1 GiB free before observation')
        if sha(ROOT / 'observer-source.py') != plan['observer_sha256'] or sha(__file__) != plan['supervisor_sha256']:
            raise RuntimeError('frozen observer source changed')
        # Older baselines used the app version for both binaries.
        daemon_version = plan.get('daemon_version', plan['version'])
        sampler = observer.PsSampler(plan['daemon_pid'])
        identity = sampler.identity()
        if identity != observer.Identity('/usr/local/bin/peterfand', plan['daemon_started']):
            raise RuntimeError('helper launch identity changed')
        app_identity = observer.PsSampler(plan['app_pid']).identity()
        if app_identity != observer.Identity(plan['app_executable'], plan['app_started']):
            raise RuntimeError('app launch identity changed')
        for field, path in [('app_sha256', plan['app_executable']),
                            ('daemon_sha256', '/usr/local/bin/peterfand'),
                            ('config_sha256', plan['config_path'])]:
            if sha(path) != plan[field]:
                raise RuntimeError(field + ' baseline mismatch')
        initial = healthy_cache(daemon_version)
        result['initial_control_health'] = initial['control_health']
        assertion = subprocess.Popen(['/usr/bin/caffeinate', '-i', '-w', str(os.getpid()),
                                      '-t', str(duration + 180)], stdin=subprocess.DEVNULL,
                                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        app_log = (ROOT / 'app-observer.log').open('wb')
        env = app_observer_environment(plan)
        child = subprocess.Popen([sys.executable, str(ROOT / 'observer-source.py')], env=env,
                                 stdin=subprocess.DEVNULL, stdout=app_log,
                                 stderr=subprocess.STDOUT, close_fds=True)
        processes = {'supervisor_pid': os.getpid(), 'supervisor_started': observer.ps(os.getpid(), 'lstart='),
                     'supervisor_parent_pid': os.getppid(), 'app_observer_pid': child.pid,
                     'app_observer_started': observer.ps(child.pid, 'lstart='),
                     'caffeinate_pid': assertion.pid, 'caffeinate_started': observer.ps(assertion.pid, 'lstart='),
                     'job_label': plan['job_label']}
        save('processes.json', processes)
        first = previous = sampler.snapshot(identity)
        peak_cpu, peak_rss, peak_threads = 0., first.rss_kb, first.threads
        result['measurement_started_at'] = observer.utc_now()
        save('progress.json', result)
        with (ROOT / 'daemon-samples.csv').open('w', newline='') as samples, (ROOT / 'daemon-health.jsonl').open('w') as health_log:
            writer = csv.writer(samples)
            writer.writerow(['elapsed_seconds', 'interval_seconds', 'cpu_percent', 'rss_kb', 'threads', 'version', 'status'])
            samples.flush()
            while previous.clock - first.clock < duration:
                time.sleep(min(interval, duration - (previous.clock - first.clock)))
                if shutil.disk_usage(ROOT).free < DISK_FLOOR:
                    raise RuntimeError('disk free space below 512 MiB; observation stopped before exhaustion')
                if child.poll() not in (None, 0):
                    raise RuntimeError('app observer failed before helper observation completed')
                if assertion.poll() is not None:
                    raise RuntimeError('idle-sleep assertion exited before observation completed')
                current = sampler.snapshot(identity)
                gap = current.clock - previous.clock
                if gap > 3 * interval + 2:
                    raise RuntimeError('suspend/stall sample gap exceeded')
                cpu = observer.cpu_percent(previous.cpu_seconds, current.cpu_seconds, gap)
                version, status = ipc('version'), ipc('status')
                if version != 'ok peterfand ' + daemon_version or status != 'ok auto (macos)':
                    raise RuntimeError('helper version/status changed')
                cache = healthy_cache(daemon_version)
                result['last_control_health'] = cache['control_health']
                health_log.write(json.dumps({'elapsed_seconds': current.clock - first.clock, **cache}) + '\n')
                health_log.flush()
                if sha(plan['config_path']) != plan['config_sha256']:
                    raise RuntimeError('user configuration changed; no restore performed')
                writer.writerow([current.clock - first.clock, gap, cpu, current.rss_kb, current.threads, version, status])
                samples.flush()
                peak_cpu, peak_rss, peak_threads = max(peak_cpu, cpu), max(peak_rss, current.rss_kb), max(peak_threads, current.threads)
                previous = current
                result['samples'] += 1
                result['elapsed_seconds'] = current.clock - first.clock
                save('progress.json', result)
            os.fsync(samples.fileno())
            os.fsync(health_log.fileno())
        measured = previous.clock - first.clock
        result['measured_seconds'] = measured
        result['daemon_metrics'] = {'average_cpu_percent': observer.cpu_percent(first.cpu_seconds, previous.cpu_seconds, measured),
                                    'peak_cpu_percent': peak_cpu, 'peak_rss_kb': peak_rss,
                                    'rss_growth_kb': previous.rss_kb - first.rss_kb,
                                    'thread_growth': peak_threads - first.threads}
        for metric, limit in LIMITS.items():
            if result['daemon_metrics'][metric] > limit:
                raise RuntimeError('helper threshold exceeded: ' + metric)
        if child.wait(timeout=45) != 0:
            raise RuntimeError('app observer failed')
        receipts = list((ROOT / 'observer').glob('menubar-*/summary.json'))
        if len(receipts) != 1:
            raise RuntimeError('missing/ambiguous app terminal receipt')
        app_summary = json.loads(receipts[0].read_text())
        result['app_summary'] = str(receipts[0])
        validate_app_summary(app_summary, duration)
        for field, path in [('app_sha256', plan['app_executable']), ('daemon_sha256', '/usr/local/bin/peterfand'), ('config_sha256', plan['config_path'])]:
            if sha(path) != plan[field]:
                raise RuntimeError(field + ' changed')
        if observer.PsSampler(plan['app_pid']).identity() != app_identity:
            raise RuntimeError('app launch identity changed')
        if plan['require_scheduled_update']:
            history = Path(plan['config_path']).with_name('update-check-history.json').read_bytes()
            (ROOT / 'update-history-final.json').write_bytes(history)
            history_value = json.loads(history)
            if history_value.get('check_error') is not None or history_value.get('last_success_unix', 0) < plan['started_unix']:
                raise RuntimeError('missing fresh successful persisted update check')
            with Path(plan['menubar_log_path']).open('rb') as stream:
                stream.seek(plan['menubar_log_offset'])
                window = stream.read()
            (ROOT / 'menubar-window.log').write_bytes(window)
            checks = re.findall(r'(\d+) v' + re.escape(plan['version']) + r' app update completed automatic=true phase=(?:current|available) channel=stable last_attempt=Some\((\d+)\) last_success=Some\((\d+)\) next_check=Some\((\d+)\)', window.decode(errors='replace'))
            fresh = [list(map(int, check)) for check in checks if int(check[2]) >= plan['started_unix'] and int(check[3]) - int(check[2]) == 21600]
            if not fresh or fresh[-1][2] != history_value['last_success_unix']:
                raise RuntimeError('native automatic update log/history/deadline mismatch')
            result['automatic_update_verified'] = {'last_success_unix': fresh[-1][2], 'next_check_unix': fresh[-1][3]}
        result['passed'] = True
    except Exception as error:
        result['errors'].append(type(error).__name__ + ': ' + str(error))
    finally:
        # Ignore repeated signals only during bounded cleanup/publication. External
        # app/helper processes are never Popen children and are never signalled.
        for number in previous_handlers:
            signal.signal(number, signal.SIG_IGN)
        for owned in (child, assertion):
            if owned is not None and owned.poll() is None:
                try:
                    owned.terminate()
                    owned.wait(timeout=15)
                except Exception as error:
                    result['passed'] = False
                    result['errors'].append('owned observer cleanup: ' + str(error))
        if app_log is not None:
            app_log.close()
        result['finished_at'] = observer.utc_now()
        published = observer.publish_terminal_result(ROOT, result, reservation, filename='combined-summary.json')
        for number, previous in previous_handlers.items():
            signal.signal(number, previous)
    return 0 if published and result['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
