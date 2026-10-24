#!/usr/bin/env python3
"""Bounded macOS process observation; never changes application configuration.

CPU is CPU-time delta / elapsed monotonic time (100% = one core). Host load is
recorded as context, not a control or a proof of performance on other machines.
This observes one process; it does not exercise OTA, hardware or user flows.
"""
import csv
import hashlib
import json
import math
import os
from pathlib import Path, PurePosixPath
import re
import signal
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from datetime import datetime, timezone


RESULT_RESERVE_BYTES = 256 * 1024
MAX_TERMINAL_RESULT_BYTES = 64 * 1024


def _sync_directory(directory):
    if os.name == 'posix':
        descriptor = os.open(directory, os.O_RDONLY | getattr(os, 'O_DIRECTORY', 0))
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)


def _write_all(descriptor, data):
    remaining = memoryview(data)
    while remaining:
        written = os.write(descriptor, remaining)
        if written <= 0:
            raise OSError('terminal storage write made no progress')
        remaining = remaining[written:]


class ResultReservation:
    def __init__(self, path, descriptor):
        self.path, self.descriptor = Path(path), descriptor
        self.released = False
        metadata = os.fstat(descriptor)
        self.identity = (metadata.st_dev, metadata.st_ino)

    def release(self):
        """Idempotently release only this run's owned reserve, never other files."""
        if self.released:
            return
        # Windows disallows unlinking an open file. The captured identity still
        # prevents this cleanup from removing a replacement at the same path.
        if self.descriptor is not None:
            descriptor, self.descriptor = self.descriptor, None
            os.close(descriptor)
        try:
            metadata = self.path.lstat()
        except FileNotFoundError:
            self.released = True
            return
        if (metadata.st_dev, metadata.st_ino) != self.identity:
            raise RuntimeError('terminal reserve identity changed; refusing removal')
        self.path.unlink()
        self.released = True
        _sync_directory(self.path.parent)


def reserve_result_space(output):
    """Reserve 256 KiB of real, fsynced storage before starting observation."""
    descriptor, path = tempfile.mkstemp(prefix='.terminal-reserve-', dir=output)
    reservation = ResultReservation(path, descriptor)
    try:
        # Random bytes avoid sparse extents and transparent zero compression.
        for _ in range(RESULT_RESERVE_BYTES // 65536):
            _write_all(descriptor, os.urandom(65536))
        os.fsync(descriptor)
        metadata = os.fstat(descriptor)
        if metadata.st_size != RESULT_RESERVE_BYTES or (
            hasattr(metadata, 'st_blocks') and metadata.st_blocks * 512 < RESULT_RESERVE_BYTES
        ):
            raise OSError('terminal reserve did not allocate its requested bytes')
        _sync_directory(output)
        return reservation
    except BaseException:
        reservation.release()
        raise


def _record_error(receipt, context, error):
    receipt['passed'] = False
    receipt.setdefault('errors', []).append(f'{context}{type(error).__name__}: {error}'[:1024])


def _stderr(message):
    try:
        print(message[:1023], file=sys.stderr, flush=True)
    except OSError:
        pass


def _terminal_path(output, filename):
    if not isinstance(filename, str) or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._-]{0,127}', filename):
        raise ValueError('terminal receipt filename must be a simple basename')
    return Path(output) / filename


def _write_terminal_result(output, receipt, *, filename='summary.json'):
    summary = _terminal_path(output, filename)
    data = (json.dumps(receipt, indent=2, allow_nan=False) + '\n').encode('utf-8')
    if len(data) > MAX_TERMINAL_RESULT_BYTES:
        raise ValueError('terminal receipt exceeds 64 KiB limit')
    descriptor, path = tempfile.mkstemp(prefix='.terminal-result-', dir=output)
    try:
        _write_all(descriptor, data)
        os.fsync(descriptor)
        os.close(descriptor)
        descriptor = None
        os.replace(path, summary)
        path = None
        _sync_directory(output)
    finally:
        if descriptor is not None:
            os.close(descriptor)
        if path is not None:
            os.unlink(path)


def publish_terminal_result(output, receipt, reservation, *, filename='summary.json'):
    """Publish atomically and durably; return persistence success, not run PASS.

    Finalization releases the reserve. Any publication error makes the run fail,
    even if the bounded retry successfully preserves its failed terminal receipt.
    Callers must check both this return value and receipt['passed'].
    """
    summary = _terminal_path(output, filename)
    if reservation is not None:
        try:
            reservation.release()
        except Exception as error:
            _record_error(receipt, 'terminal reserve release: ', error)
    for _ in range(2):
        try:
            _write_terminal_result(output, receipt, filename=filename)
            return True
        except Exception as error:
            _record_error(receipt, 'terminal receipt publication: ', error)
    # A rename may have completed before directory fsync failed. Do not leave
    # an unconfirmed positive receipt behind if the failed retry cannot replace it.
    try:
        if summary.exists() and json.loads(summary.read_text())['passed']:
            summary.unlink()
            _sync_directory(output)
    except Exception as error:
        _record_error(receipt, 'unconfirmed terminal receipt removal: ', error)
    _stderr('FAIL terminal receipt could not be persisted: ' + receipt['errors'][-1])
    return False


def bounded(env, key, default, low, high, integer=False):
    raw = env.get(key, str(default))
    if integer and not re.fullmatch(r"[0-9]+", raw):
        raise ValueError(f"{key} must be an integer")
    value = int(raw) if integer else float(raw)
    if not math.isfinite(value) or not low <= value <= high:
        raise ValueError(f"{key} must be between {low} and {high}")
    return value


@dataclass
class Settings:
    duration: int
    interval: int
    pid: object
    limits: dict
    log_dir: Path
    app: Path

    @classmethod
    def from_env(cls, env):
        root = Path(__file__).resolve().parent.parent
        duration = bounded(env, 'PETERFAN_SOAK_SECONDS', 21600, 30, 604800, True)
        interval = bounded(env, 'PETERFAN_SOAK_SAMPLE_SECONDS', 10, 1, min(3600, duration), True)
        pid = None
        if env.get('PETERFAN_SOAK_PID'):
            pid = bounded(env, 'PETERFAN_SOAK_PID', 0, 2, 2147483647, True)
            if pid == os.getpid():
                raise ValueError('PETERFAN_SOAK_PID cannot be the observer')
        limits = {
            'average_cpu_percent': bounded(env, 'PETERFAN_SOAK_MAX_AVG_CPU', 8, 0, 100000),
            'peak_cpu_percent': bounded(env, 'PETERFAN_SOAK_MAX_PEAK_CPU', 35, 0, 100000),
            'peak_rss_kb': bounded(env, 'PETERFAN_SOAK_MAX_RSS_KB', 262144, 1, 2**40, True),
            'rss_growth_kb': bounded(env, 'PETERFAN_SOAK_MAX_RSS_GROWTH_KB', 65536, 0, 2**40, True),
            'thread_growth': bounded(env, 'PETERFAN_SOAK_MAX_THREAD_GROWTH', 16, 0, 1000000, True),
        }
        return cls(duration, interval, pid, limits,
                   Path(env.get('PETERFAN_SOAK_LOG_DIR', str(root / 'target/soak'))),
                   Path(env.get('PETERFAN_MENUBAR_BIN', str(root / 'target/release/peterfan-menubar'))).resolve())


def parse_cpu_time(value):
    match = re.fullmatch(r'(?:(\d+)-)?(?:(\d+):)?(\d+):(\d+(?:\.\d+)?)', value)
    if not match:
        raise ValueError(f'invalid cumulative CPU time: {value!r}')
    days, hours, minutes, seconds = match.groups()
    # ps may retain cumulative minutes beyond 59 in MM:SS output. In
    # HH:MM:SS / DD-HH:MM:SS, minutes remain a bounded subfield.
    if (hours is not None and int(minutes) >= 60) or float(seconds) >= 60:
        raise ValueError('invalid CPU time fields')
    if days is not None and (hours is None or int(hours) >= 24):
        raise ValueError('invalid day/hour CPU time fields')
    return int(days or 0) * 86400 + int(hours or 0) * 3600 + int(minutes) * 60 + float(seconds)


def is_menu_bar_executable(executable):
    path = PurePosixPath(executable)
    if not path.is_absolute():
        return False
    if path.name == 'peterfan-menubar':
        return True
    return (path.name == 'PeterFan' and path.parent.name == 'MacOS'
            and path.parent.parent.name == 'Contents'
            and path.parent.parent.parent.name == 'PeterFan.app')


def cpu_percent(before, after, elapsed):
    if not all(math.isfinite(x) for x in (before, after, elapsed)) or after < before or elapsed <= 0:
        raise ValueError('CPU time regressed or sample interval was invalid')
    return 100 * (after - before) / elapsed


def observation_clock():
    # Darwin monotonic()/uptime pauses during suspend. This public raw clock
    # includes sleep without wall-clock/NTP jumps, so gaps invalidate the run.
    # Fail closed if the supported native clock cannot be read.
    if sys.platform == 'darwin':
        return time.clock_gettime(time.CLOCK_MONOTONIC_RAW)
    return time.monotonic()


def ps(pid, columns):
    result = subprocess.run(['/bin/ps', '-ww', '-p', str(pid), '-o', columns],
                            capture_output=True, text=True, timeout=10, check=True,
                            env={**os.environ, 'LC_ALL': 'C'})
    return result.stdout.strip()


def thread_count(pid):
    result = subprocess.run(['/bin/ps', '-M', '-p', str(pid)],
                            capture_output=True, text=True, timeout=10, check=True,
                            env={**os.environ, 'LC_ALL': 'C'})
    # macOS ps -M ignores -o and emits its own thread table. Continuation
    # rows omit USER/TT, so validate the PID in each row rather than counting
    # an error/header as a thread or assuming a fixed username width.
    lines = result.stdout.splitlines()
    if not lines or lines[0].split()[:2] != ['USER', 'PID']:
        raise RuntimeError('missing process thread header')
    rows = [line.split() for line in lines[1:]]
    if not rows or len(rows[0]) < 2 or rows[0][1] != str(pid):
        raise RuntimeError('missing primary process thread')
    if any(not row or row[0] != str(pid) for row in rows[1:]):
        raise RuntimeError('missing or malformed process thread sample')
    return len(rows)


@dataclass(frozen=True)
class Identity:
    executable: str
    started: str


@dataclass
class Snapshot:
    clock: float
    cpu_seconds: float
    rss_kb: int
    threads: int


class PsSampler:
    def __init__(self, pid):
        self.pid = pid

    def identity(self):
        executable = ps(self.pid, 'comm=')
        started = ps(self.pid, 'lstart=')
        if not executable.startswith('/') or not started or '\n' in executable or '\n' in started:
            raise RuntimeError('cannot establish executable and launch identity')
        return Identity(executable, started)

    def snapshot(self, pinned):
        if self.identity() != pinned:
            raise RuntimeError('process identity changed before sample')
        fields = ps(self.pid, 'time=,rss=').split()
        if len(fields) != 2:
            raise RuntimeError('missing or malformed CPU/RSS sample')
        cpu, rss = parse_cpu_time(fields[0]), int(fields[1])
        threads = thread_count(self.pid)
        if rss <= 0 or threads <= 0:
            raise RuntimeError('non-positive RSS/thread sample')
        if self.identity() != pinned:
            raise RuntimeError('process identity changed during sample')
        return Snapshot(observation_clock(), cpu, rss, threads)


def config_fingerprint(path):
    try:
        return {'exists': True, 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}
    except FileNotFoundError:
        # A dangling symlink is not equivalent to a previously absent file.
        if path.is_symlink():
            raise RuntimeError('configuration is a dangling symlink')
        return {'exists': False, 'sha256': None}


def guarded_terminate(proc, pinned, identity):
    # An external PID has no Popen owner. Never signal it, including on failure.
    if proc is None or proc.poll() is not None or pinned is None:
        return
    if identity() != pinned:
        raise RuntimeError('owned process identity changed; refusing termination')
    proc.terminate()
    try:
        proc.wait(timeout=10)
    except subprocess.TimeoutExpired:
        # Do not escalate to SIGKILL or accidentally target a reused PID.
        raise RuntimeError('owned process did not stop within 10 seconds')


def utc_now():
    return datetime.now(timezone.utc).isoformat()


def run(settings):
    settings.log_dir.mkdir(parents=True, exist_ok=True)
    output = Path(tempfile.mkdtemp(prefix='menubar-', dir=settings.log_dir))
    config_path = Path.home() / 'Library/Application Support/peterfan/config.toml'
    receipt = {
        'schema_version': 1, 'started_at': utc_now(), 'passed': False,
        'scope': 'one macOS process; interval CPU, RSS and threads; not hardware/OTA/user-flow acceptance',
        'cpu_semantics': 'cumulative process CPU delta / monotonic elapsed; 100 percent = one core',
        'host_context': {'logical_cpu_count': os.cpu_count(), 'load_average_start': os.getloadavg()},
        'requested_seconds': settings.duration, 'sample_interval_seconds': settings.interval,
        'thresholds': settings.limits, 'samples': 0, 'errors': [],
        'config_path': str(config_path), 'owned_process': settings.pid is None,
    }
    proc, pinned, sampler = None, None, None
    reservation, published = None, False
    previous_handlers = {}
    config_before = None
    def interrupted(signum, _frame):
        raise KeyboardInterrupt(f'soak interrupted by {signal.Signals(signum).name}')
    try:
        for signum in (signal.SIGTERM, signal.SIGINT):
            previous_handlers[signum] = signal.signal(signum, interrupted)
        config_before = config_fingerprint(config_path)
        receipt['config_before'] = config_before
        reservation = reserve_result_space(output)
        if settings.pid is None:
            if not settings.app.is_file() or not os.access(settings.app, os.X_OK):
                raise RuntimeError(f'{settings.app} is not executable; build release first')
            with (output / 'application.log').open('wb') as log:
                proc = subprocess.Popen([str(settings.app), '--mock'], stdout=log, stderr=subprocess.STDOUT)
            pid = proc.pid
        else:
            pid = settings.pid
        receipt['pid'] = pid
        sampler = PsSampler(pid)
        pinned = sampler.identity()
        receipt['identity'] = {'executable': pinned.executable, 'started': pinned.started}
        if not is_menu_bar_executable(pinned.executable):
            raise RuntimeError('PID is not the PeterFan menu-bar executable')
        if proc is not None and Path(pinned.executable).resolve() != settings.app:
            raise RuntimeError('launched executable does not match requested binary')
        # Exclude startup settling from the measurement; identity is already pinned.
        if proc is not None:
            time.sleep(5)
        first = previous = sampler.snapshot(pinned)
        peak_cpu = 0.0
        peak_rss, peak_threads = first.rss_kb, first.threads
        with (output / 'samples.csv').open('w', newline='') as csvfile:
            writer = csv.writer(csvfile)
            writer.writerow(['elapsed_seconds', 'interval_seconds', 'cpu_percent', 'cpu_seconds', 'rss_kb', 'threads', 'host_load_1m'])
            csvfile.flush()
            os.fsync(csvfile.fileno())
            while previous.clock - first.clock < settings.duration:
                remaining = settings.duration - (previous.clock - first.clock)
                time.sleep(min(settings.interval, remaining))
                current = sampler.snapshot(pinned)
                elapsed = current.clock - previous.clock
                # A sleep, stalled ps or scheduler gap invalidates peak evidence.
                if elapsed > settings.interval * 3 + 2:
                    raise RuntimeError('sample gap exceeded bound; suspend/stall invalidates this run')
                cpu = cpu_percent(previous.cpu_seconds, current.cpu_seconds, elapsed)
                writer.writerow([round(current.clock-first.clock, 6), round(elapsed, 6), round(cpu, 6), current.cpu_seconds, current.rss_kb, current.threads, os.getloadavg()[0]])
                csvfile.flush()
                os.fsync(csvfile.fileno())
                peak_cpu = max(peak_cpu, cpu)
                peak_rss, peak_threads = max(peak_rss, current.rss_kb), max(peak_threads, current.threads)
                receipt['samples'] += 1
                previous = current
        if receipt['samples'] < 1:
            raise RuntimeError('no complete CPU intervals sampled')
        duration = previous.clock - first.clock
        metrics = {
            'average_cpu_percent': cpu_percent(first.cpu_seconds, previous.cpu_seconds, duration),
            'peak_cpu_percent': peak_cpu, 'peak_rss_kb': peak_rss,
            'rss_growth_kb': previous.rss_kb-first.rss_kb,
            # Use peak growth, so transient thread leaks cannot hide at final sample.
            'thread_growth': peak_threads-first.threads,
        }
        receipt.update(measured_seconds=duration, metrics=metrics)
        for key, limit in settings.limits.items():
            if metrics[key] > limit:
                receipt['errors'].append(f'{key}: {metrics[key]:.3f} > {limit}')
    except (Exception, KeyboardInterrupt) as error:
        _record_error(receipt, '', error)
    finally:
        # The first termination request fails the observation; repeated signals
        # must not interrupt its bounded terminal publication and cleanup.
        for signum in previous_handlers:
            signal.signal(signum, signal.SIG_IGN)
        try:
            guarded_terminate(proc, pinned, sampler.identity if sampler else lambda: None)
        except Exception as error:
            _record_error(receipt, 'cleanup: ', error)
        try:
            after = config_fingerprint(config_path)
            receipt['config_after'] = after
            receipt['config_preserved'] = config_before is not None and after == config_before
            if not receipt['config_preserved']:
                receipt['errors'].append('configuration changed during observation; no restore attempted')
        except Exception as error:
            _record_error(receipt, 'configuration verification: ', error)
        receipt['finished_at'] = utc_now()
        try:
            receipt['host_context']['load_average_end'] = os.getloadavg()
        except Exception as error:
            _record_error(receipt, 'final host sample: ', error)
        receipt['passed'] = not receipt['errors'] and receipt['samples'] > 0
        try:
            published = publish_terminal_result(output, receipt, reservation)
        finally:
            for signum, previous in previous_handlers.items():
                signal.signal(signum, previous)
    passed = published and receipt['passed']
    try:
        print(f"{'PASS' if passed else 'FAIL'} PeterFan macOS soak: {output / 'summary.json'}", flush=True)
    except OSError:
        pass
    if receipt['errors']:
        _stderr('\n'.join(f'  {error}' for error in receipt['errors']))
    return 0 if passed else 1


def main():
    if sys.platform != 'darwin':
        print('error: the menu-bar soak test requires macOS', file=sys.stderr)
        return 2
    if len(sys.argv) != 1:
        print('error: use PETERFAN_SOAK_* environment variables; positional arguments are unsupported', file=sys.stderr)
        return 2
    try:
        settings = Settings.from_env(os.environ)
    except (ValueError, OverflowError) as error:
        print(f'error: {error}', file=sys.stderr)
        return 2
    return run(settings)


if __name__ == '__main__':
    sys.exit(main())
