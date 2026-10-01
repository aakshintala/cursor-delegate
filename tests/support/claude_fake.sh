#!/bin/sh
case "$1" in
  --version) echo "2.0.0-test" ;;
  auth)
    if [ "$2" = status ]; then
      printf '%s\n' '{"loggedIn":true}'
    else
      echo "unexpected auth $*" >&2
      exit 1
    fi
    ;;
  *) echo "unexpected: $*" >&2; exit 1 ;;
esac
