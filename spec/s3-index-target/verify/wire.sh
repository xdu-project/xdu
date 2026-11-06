#!/bin/sh
# A pointer, kept so this spec's recorded phase gates still run: the S3 wire gate moved to
# tests/s3-wire.sh when review cycle 1 made it a standing CI gate. That script is the one to
# change; this one only hands over its arguments.
exec sh tests/s3-wire.sh "$@"
