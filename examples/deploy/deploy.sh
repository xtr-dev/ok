#!/usr/bin/env bash
# Build the frontend and deploy it to staging or production.
set -euo pipefail
ENVIRONMENT="${1:?usage: deploy.sh <staging|production>}"
case "$ENVIRONMENT" in
  staging)    HOST=staging.example.com ;;
  production) HOST=app.example.com ;;
  *) echo "unknown environment: $ENVIRONMENT" >&2; exit 2 ;;
esac
VERSION=$(git describe --tags --always)

if [[ "$ENVIRONMENT" == "production" ]]; then
  read -r -p "Deploy $VERSION to PRODUCTION ($HOST)? [y/N] " answer
  [[ "$answer" == "y" || "$answer" == "Y" ]] || { echo "aborted"; exit 1; }
fi

npm ci
npm run build
ARCHIVE="/tmp/app-$VERSION.tar.gz"   # /tmp does not exist on Windows
trap 'rm -f "$ARCHIVE"' EXIT
tar -czf "$ARCHIVE" dist
scp "$ARCHIVE" "deploy@$HOST:/srv/app/releases/"
ssh "deploy@$HOST" "cd /srv/app && ./activate.sh $VERSION"
curl -fsS "https://$HOST/health" > /dev/null
echo "deployed $VERSION to $ENVIRONMENT"
