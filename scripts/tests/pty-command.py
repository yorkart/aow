"""Run a shell command with a controlling TTY on macOS or Linux (tests only)."""
import errno
import fcntl
import os
import pty
import select
import sys
import termios

reply = sys.stdin.buffer.read()
master, slave = pty.openpty()
attributes = termios.tcgetattr(slave)
attributes[3] &= ~termios.ECHO
termios.tcsetattr(slave, termios.TCSANOW, attributes)
pid = os.fork()
if pid == 0:
    os.close(master)
    os.setsid()
    fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
    for fd in (0, 1, 2):
        os.dup2(slave, fd)
    if slave > 2:
        os.close(slave)
    os.execvp('bash', ['bash', '-c', sys.argv[1]])
os.close(slave)
os.write(master, reply or b'\x04')
while True:
    try:
        ready, _, _ = select.select([master], [], [], 0.1)
        if ready:
            data = os.read(master, 65536)
            if not data:
                break
            sys.stdout.buffer.write(data)
            sys.stdout.buffer.flush()
    except OSError as error:
        if error.errno != errno.EIO:
            raise
        break
os.close(master)
_, status = os.waitpid(pid, 0)
sys.exit(os.WEXITSTATUS(status) if os.WIFEXITED(status) else 128 + os.WTERMSIG(status))
