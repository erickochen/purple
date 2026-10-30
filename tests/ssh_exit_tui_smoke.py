#!/usr/bin/env python3
"""Exercise actual TUI reconnect/exit notifications with a deterministic SSH launcher."""
import fcntl, os, pathlib, pty, re, resource, select, struct, subprocess, sys, tempfile, termios, time
binary = str(pathlib.Path(sys.argv[1]).resolve())
for code in [0, 1, 130, 255]:
    print(f'TUI case exit={code}', flush=True)
    with tempfile.TemporaryDirectory(prefix='purple-exit-') as tmp:
        root=pathlib.Path(tmp);state=root/'state';state.mkdir()
        (state/'history.tsv').write_text('')
        (state/'last_version_check').write_text(f'{int(time.time())}\n0.0.0\n\n')
        config=root/'config';config.write_text('Host exit-test\n HostName 127.0.0.1\n User nobody\n')
        marker=root/'called'
        launcher=root/'ssh'
        launcher.write_text('#!/usr/bin/env python3\nimport pathlib,sys\n'+f'pathlib.Path({str(marker)!r}).touch()\n'+
            'print("Warning: Permanently added \'test\' (ED25519) to the list of known hosts.",file=sys.stderr)\n'+
            ('print("Permission denied (password).",file=sys.stderr)\n' if code==255 else '')+f'sys.exit({code})\n')
        launcher.chmod(0o755)
        # A temp HOME keeps the first-run migration away from the real ~/.purple.
        env=dict(os.environ,HOME=str(root),TERM='xterm-256color',PURPLE_SSH_COMMAND=str(launcher))
        env.pop('TMUX',None)
        for category in ['CONFIG','DATA','STATE','CACHE']:env['PURPLE_'+category+'_DIR']=str(state)
        master,slave=pty.openpty();fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',45,240,0,0))
        def session():
            resource.setrlimit(resource.RLIMIT_CORE,(0,0))
            os.setsid();fcntl.ioctl(0,termios.TIOCSCTTY,0)
        child=subprocess.Popen([binary,'--config',str(config)],stdin=slave,stdout=slave,stderr=slave,env=env,preexec_fn=session)
        os.close(slave);data=b'';sent=False;called_at=None;deadline=time.monotonic()+15
        try:
            while time.monotonic()<deadline:
                if select.select([master],[],[],.1)[0]:
                    try:chunk=os.read(master,65536)
                    except OSError:break
                    data+=chunk
                    if b'\x1b[6n' in chunk:os.write(master,b'\x1b[1;1R')
                if not sent and b'exit-test' in data and not termios.tcgetattr(master)[3]&termios.ECHO:
                    os.write(master,b'\r');sent=True
                if marker.exists() and called_at is None:called_at=time.monotonic()
                if called_at and time.monotonic()-called_at>1:break
            assert marker.exists(), 'TUI never connected: '+data.decode(errors='replace')[-2000:]
            # Only inspect the resumed alternate screen, not raw SSH stderr.
            assert data.count(b'\x1b[?1049h')>=2, 'TUI did not resume'
            resumed=data.rsplit(b'\x1b[?1049h',1)[-1]
            text=re.sub(rb'\x1b\[[0-?]*[ -/]*[@-~]',b'',resumed).decode(errors='replace')
            flat=''.join(text.split())
            if code==255:
                assert 'SSHtoexit-testfailed.' in flat and 'Permissiondenied' in flat,text[-2500:]
                assert 'Permanentlyadded' not in flat,text[-2500:]
            else:assert 'SSHtoexit-testfailed.' not in flat,text[-2500:]
            assert 'exit-test' in text, text[-2000:]
            os.write(master,b'q')
            deadline=time.monotonic()+3
            while child.poll() is None and time.monotonic()<deadline:
                if select.select([master],[],[],.1)[0]:
                    try:chunk=os.read(master,65536)
                    except OSError:break
                    if b'\x1b[6n' in chunk:os.write(master,b'\x1b[1;1R')
        finally:
            os.close(master)
            if child.poll() is None:child.kill()
            child.wait(timeout=3)
print('PASS: real TUI does not report shell exit 0/1/130 as SSH failure; exit 255 retains the real error without the known-hosts notice.')
