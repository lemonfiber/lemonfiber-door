FROM caddy:2.11.7@sha256:685ae3d7c34d9b5f8caeddc5d14051db7bbd88ff1f5a2fa0c16bd8ca328fb7af
RUN apk add --no-cache libcap \
 && setcap -r /usr/bin/caddy \
 && apk del --no-cache libcap
USER 1000:1000
