#!/bin/bash

sqlcipher -stats -column -echo DATABASE.SQLITE <<EOF
pragma key = '0000000000000000000000000000000000000000000000000000000000000000';
.schema
SELECT * FROM entries;
EOF