FROM tusproject/tusd:v2.10.1@sha256:7b1c552a8b42f4b36cb01f2a3bd49f82ab078b2eefd191459e716141dd50376c
USER root
RUN chown -R 10001:10001 /srv/tusd-data
COPY --chmod=755 pre-create /srv/tusd-hooks/pre-create
USER 10001:10001
