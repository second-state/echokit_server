This doc guides you through building and deploying the echokit_server container to Fly.io.

## Prerequisites
- flyctl installed: see https://github.com/superfly/flyctl
- Docker installed and running locally
- A unique Fly app name (Fly app names are globally unique)

## 1) Create the Fly app
```bash
fly app create --name echokit-server-[something-unique]
```

## 2) Build the Docker image
From the server-vad directory, ensure `config.toml` is valid, then build:
```bash
cd server-vad
docker build -t registry.fly.io/[your_app_name]:tag .
```

## 3) Push the image to Fly
Authenticate to the Fly registry and push the image:
```bash
fly auth docker
docker push registry.fly.io/[your_app_name]:tag
```

## 4) Configure Fly to use the image
Update `fly.toml` to point to `registry.fly.io/[your_app_name]:tag`.

## 5) Deploy
From the `docker` directory:
```bash
fly deploy
```

## Managing machines
- Deploy creates two machines in the same region by default. Remove one if you only need a single instance:
  ```bash
  fly machine list           # get machine IDs
  fly machine destroy [id]   # remove one
  ```
- To run in additional regions, clone a machine:
  ```bash
  fly platform regions                   # list region codes
  fly machine clone [id] --region dfw    # example clone into DFW
  ```

## Quick checklist
- [ ] flyctl installed and authenticated
- [ ] Unique app name chosen
- [ ] `config.toml` validated in `server-vad`
- [ ] Image built and pushed to Fly registry
- [ ] `fly.toml` updated with the pushed image tag
- [ ] App deployed with `fly deploy`
