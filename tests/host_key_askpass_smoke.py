#!/usr/bin/env python3
"""Real OpenSSH + temporary localhost sshd; no user keys, passwords or known_hosts."""
import fcntl, os, pathlib, pty, select, socket, subprocess, sys, tempfile, termios, time
binary = str(pathlib.Path(sys.argv[1]).resolve())
with tempfile.TemporaryDirectory(prefix='purple-host-key-') as tmp:
    root = pathlib.Path(tmp)
    def key(name):
        p = root/name
        subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(p)],check=True)
        return p
    hostkey = key('host'); replacement = key('replacement')
    sock=socket.socket();sock.bind(('127.0.0.1',0));port=sock.getsockname()[1];sock.close()
    config=root/'sshd_config'
    config.write_text(f'Port {port}\nListenAddress 127.0.0.1\nHostKey {hostkey}\nPidFile {root}/pid\nUsePAM no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nPubkeyAuthentication no\n')
    server=subprocess.Popen(['/usr/sbin/sshd','-D','-e','-f',str(config)],stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
    try:
        for _ in range(50):
            if server.poll() is not None: raise AssertionError(server.stderr.read().decode())
            try:
                with socket.create_connection(('127.0.0.1',port),timeout=.1):break
            except OSError:time.sleep(.05)
        known=root/'known_hosts'
        env=dict(os.environ,SSH_ASKPASS=binary,SSH_ASKPASS_REQUIRE='force',PURPLE_ASKPASS_MODE='1',PURPLE_HOST_ALIAS='test',PURPLE_CONFIG_PATH=str(root/'config'))
        for category in ['CONFIG','DATA','STATE','CACHE']:env['PURPLE_'+category+'_DIR']=str(root/'purple')
        (root/'config').write_text('Host test\n HostName 127.0.0.1\n')
        cmd=['ssh','-T','-F','/dev/null','-p',str(port),'-o',f'UserKnownHostsFile={known}','-o','GlobalKnownHostsFile=/dev/null','-o','StrictHostKeyChecking=ask','-o','ConnectTimeout=5','-o','PasswordAuthentication=no','-o','PubkeyAuthentication=no','-o','KbdInteractiveAuthentication=no','127.0.0.1','true']
        def run(answer=None, background=False):
            master,slave=pty.openpty()
            def session():
                os.setsid();fcntl.ioctl(0,termios.TIOCSCTTY,0)
            child_env=dict(env)
            if background:child_env['PURPLE_ASKPASS_SINGLE_ATTEMPT']='1'
            p=subprocess.Popen(cmd,env=child_env,stdin=slave,stdout=slave,stderr=slave,preexec_fn=session)
            os.close(slave);data=b'';sent=False;deadline=time.monotonic()+12
            while time.monotonic()<deadline:
                if select.select([master],[],[],.1)[0]:
                    try:chunk=os.read(master,65536)
                    except OSError:break
                    if not chunk:break
                    data+=chunk
                    if b'yes/no' in data and answer is not None and not sent:
                        os.write(master,answer);sent=True
                if p.poll() is not None:break
            try:p.wait(timeout=1)
            except subprocess.TimeoutExpired:
                p.kill();p.wait();raise AssertionError('prompt stalled: '+data.decode(errors='replace'))
            finally:os.close(master)
            return data,sent
        data,sent=run(b'no\n');assert sent and b'SHA256:' in data, data
        assert not known.exists() or not known.read_text().strip()
        data,sent=run(background=True);assert not sent and b'Host key verification failed' in data,data
        assert not known.exists() or not known.read_text().strip()
        data,sent=run(b'SHA256:\n');assert sent and b'Host key verification failed' in data,data
        assert not known.exists() or not known.read_text().strip()
        data,sent=run(b'Yes\n');assert sent and known.read_text().strip(),data
        data,sent=run();assert b'yes/no' not in data and b'Host key verification failed' not in data,data
        # An already trusted but different key must never enter the yes/no path.
        pub=replacement.with_suffix('.pub').read_text().split()
        known.write_text(f'[127.0.0.1]:{port} {pub[0]} {pub[1]}\n')
        before=known.read_bytes();data,sent=run()
        assert b'REMOTE HOST IDENTIFICATION HAS CHANGED' in data,data
        assert not sent and before==known.read_bytes()
        print('PASS: real OpenSSH shows fingerprint; decline and background refuse; explicit yes trusts; reconnect skips confirmation; changed key remains rejected.')
    finally:
        server.terminate();server.wait(timeout=5)
