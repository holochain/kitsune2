#!/bin/sh

test-auth-hook-server 8080 &
kitsune2-bootstrap-srv --production --tls-cert /etc/bootstrap_srv/cert.pem --tls-key /etc/bootstrap_srv/key.pem --authentication-hook-server http://127.0.0.1:8080/authenticate
