FROM caddy:2.11.6@sha256:3422ce6de165df66534f9b9ba50efaf457114ec961763cc52f5dbdaac2972d73
RUN apk add --no-cache libcap \
 && setcap -r /usr/bin/caddy \
 && apk del --no-cache libcap
USER 1000:1000
