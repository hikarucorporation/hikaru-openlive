#!/bin/bash

# Guardamos el total de líneas en una variable y extraemos solo el número
TOTAL_LINES=$(git ls-files -z | xargs -0 -r wc -l 2>/dev/null | tail -n 1 | awk '{print $1}')

echo "Hikaru OpenLive CLI | Code Lines:"
echo "$TOTAL_LINES"