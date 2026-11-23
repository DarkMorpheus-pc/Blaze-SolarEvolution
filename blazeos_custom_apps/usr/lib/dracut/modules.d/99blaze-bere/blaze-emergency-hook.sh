#!/usr/bin/sh
if [ -x /usr/bin/blaze-death-screen ]; then
    /usr/bin/blaze-death-screen </dev/tty1 >/dev/tty1 2>&1
fi
