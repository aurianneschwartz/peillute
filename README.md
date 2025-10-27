# Peillute - A Distributed Cross-Platform Payment App in Rust

This project aims to recreate "Pay'UTC" as a distributed application in Rust using TCP for communication between nodes. The goal is to maintain a common database on each node using what we learned through the course "SR05" at Université de Technologie de Compiègne.

Each node can be launched independently; they will automatically connect to other instances present in the network and start exchanging data. We use vector clocks to ensure that transactions are processed in the right order, and we also use a snapshot mechanism to ensure that the database is always consistent.

Through this project, we also wanted to create a "production-like" application with all the learning from our internship. That's why we implemented automatic tests, documentation generation, and CI/CD pipelines.

Peillute is available as a terminal application but can also be accessed as a web app using [Dioxus](https://dioxuslabs.com/).

Dioxus allows us to create a web application with a single binary that contains both the server (database, networking, etc.) and the client (UI).

[TOC]

<p align="center">
  <img src="assets/doc/peillute_pay_page.jpeg" alt="Pay page" width="49%">
  <img src="assets/doc/peillute_system_info.jpeg" alt="System Info" width="49%">
</p>

## Project Documentation

The project documentation is automatically generated using `cargo doc` and deployed using the CI/CD pipeline. You can find it here: [peillute](https://guegathe.gitlab.utc.fr/peillute/doc/peillute/)

## 🚀 Installation

### Observability

To deploy the complete demonstration environment with observability, follow these steps:

#### Build and Deployment

1. **Build the Docker image**:
```bash
   docker build -t peillute:latest .
```

2. **Launch the environment with Docker Compose**:
```bash
   docker-compose up
```

This will create a network of 3 interconnected nodes: `n1 ↔ n2 ↔ n3`

#### Observability Stack

The application integrates a complete observability stack:
- **Loki**: log aggregation and management
- **Jaeger**: distributed request tracing
- **Grafana**: metrics and logs visualization

#### Access Points

Once deployment is complete, services will be accessible at the following addresses:

| Service | URL | Credentials |
|---------|-----|-------------|
| Node 1 | http://localhost:8080 | - |
| Node 2 | http://localhost:8081 | - |
| Node 3 | http://localhost:8082 | - |
| Jaeger | http://localhost:16686 | - |
| Grafana | http://localhost:3000 | admin / admin |

> **Note**: Loki is not directly accessible but is connected to Grafana for log visualization.

Feel free to explore this sandboxed environment!


### Installation with Docker

To set up the application using Docker, follow these steps:

#### 1. Build the Docker image
```bash
docker build -t peillute .

To set up the application using Docker, follow these steps:

```bash
docker build -t peillute .
```

#### 2. Run the container

You can start the container in either interactive or detached mode:

- Interactive mode (console)

```bash
docker run -it --rm --name peillute1 \
  -e CLI_IP=0.0.0.0 \
  -e CLI_PORT=10000 \
  -p 10000:10000 \
  -p 11001:11001 \
  peillute:latest
```

- Detached mode (background)

```bash
docker run -d --name peillute1 \
  -e CLI_IP=0.0.0.0 \
  -e CLI_PORT=10000 \
  -p 10000:10000 \
  -p 11001:11001 \
  peillute:latest
```

> Note: Once the container is running, the web application will be available at: http://localhost:11001

### Docker Compose Deployment

#### Test Scenario
Deploy a three-node distributed network with the following topology:
```
n1 ↔ n2 ↔ n3
```

Where:
- **n1**: Node 1
- **n2**: Node 2 (central node)
- **n3**: Node 3

#### Commands

**Start the network:**
```bash
docker-compose up -d
```

**Stop and remove the network:**
```bash
docker-compose down
```

#### Network Configuration
Each node is automatically configured with:
- Unique IP addresses on the Docker bridge network
- Peer connections to adjacent nodes
- Web interface accessible on ports 8080, 8081, and 8082 respectively


### Prerequisites

Make sure you have the following installed on your system:
- Rust
- Cargo
- Dioxus

#### Chromium

Dioxus seems to have some problems running on Firefox, please use a Chromium based browser.
Safari seems to work fine as well.

e.g. Google Chrome

### 1. Clone the Repository

```sh
git clone https://gitlab.utc.fr/guegathe/peillute.git -j8
```

### 2. Automatically Install Dependencies and Run Peillute Instance

```sh
# the install flag is only necessary on Linux (debian or fedora based) to install some apt packages needed for Dioxus
./launch_peillute_instance.sh -install

# To be more verbose:
./launch_peillute_instance.sh -debug

# To run in CLI mode without UI:
./launch_peillute_instance.sh -cli

# To run in CLI mode with debug logs:
./launch_peillute_instance.sh -debug -cli

# To run a demo using Dioxus:
./launch_peillute_instance.sh -demo

# To run a demo using Dioxus with debug logs:
./launch_peillute_instance.sh -debug -demo

# To run a demo using CLI mode:
./launch_peillute_instance.sh -demo_cli

# To run a demo using CLI mode with debug logs:
./launch_peillute_instance.sh -debug -demo_cli
```

### 3. Manually Install Dependencies

If you prefer to install dependencies manually, follow these steps:

#### Install Rust

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

#### Install Cargo bin-install

```sh
curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
```

#### Install Dioxus

```sh
cargo binstall dioxus-cli
```

#### For Linux and Windows Users

Refer to the [Dioxus Getting Started Guide](https://dioxuslabs.com/learn/0.6/getting_started/#) for additional setup instructions.

## 🚀 Compile and Run

### 1. Compile and Run Without the UI

```sh
cargo run
```

### Demonstration of Imperfect Network

The following commands will create a non-perfect network (schema below) with manual peers:

<p align="center">
  <img src="assets/doc/peillute_network.png" alt="Network" width="80%">
</p>

```sh
# Create a non-perfect network with manual peers:
# Terminal 1
RUST_LOG=debug cargo run -- --cli-port 10000 --cli-peers 127.0.0.1:10001,127.0.0.1:10003 --cli-db-id 0
# Terminal 2
RUST_LOG=debug cargo run -- --cli-port 10001 --cli-peers 127.0.0.1:10000,127.0.0.1:10002,127.0.0.1:10004 --cli-db-id 1
# Terminal 3
RUST_LOG=debug cargo run -- --cli-port 10002 --cli-peers 127.0.0.1:10001,127.0.0.1:10003 --cli-db-id 2
# Terminal 4
RUST_LOG=debug cargo run -- --cli-port 10003 --cli-peers 127.0.0.1:10000,127.0.0.1:10002 --cli-db-id 3
# Terminal 5
RUST_LOG=debug cargo run -- --cli-port 10004 --cli-peers 127.0.0.1:10001,127.0.0.1:10006,127.0.0.1:10005 --cli-db-id 4
# Terminal 6
RUST_LOG=debug cargo run -- --cli-port 10005 --cli-peers 127.0.0.1:10004,127.0.0.1:10009 --cli-db-id 5
# Terminal 7
RUST_LOG=debug cargo run -- --cli-port 10006 --cli-peers 127.0.0.1:10004,127.0.0.1:10007,127.0.0.1:10008 --cli-db-id 6
# Terminal 8
RUST_LOG=debug cargo run -- --cli-port 10007 --cli-peers 127.0.0.1:10006,127.0.0.1:10008 --cli-db-id 7
# Terminal 9
RUST_LOG=debug cargo run -- --cli-port 10008 --cli-peers 127.0.0.1:10006,127.0.0.1:10007 --cli-db-id 8
# Terminal 10
RUST_LOG=debug cargo run -- --cli-port 10009 --cli-peers 127.0.0.1:10005 --cli-db-id 9
```

### 2. Compile with Dioxus (Merges Client and Server)

Dioxus is a full-stack cross-platform framework, so Peillute can be deployed on:

- Linux (Desktop)
- MacOS (Desktop)
- Windows (Desktop)
- Web (Browser)
- Android (Mobile)
- iOS (Mobile)

To compile Peillute with Dioxus, run the following command:

```sh
dx bundle --release --platform web
```

### 3. Run the Binary

Manually run the server:

```sh
# One instance
cd target/dx/peillute/release/web
./server

# Create a non-perfect network with manual peers:
# Terminal 1:
RUST_LOG=debug ./server --cli-port 10000 --cli-peers 127.0.0.1:10001,127.0.0.1:10002
# Terminal 2:
RUST_LOG=debug ./server --cli-port 10001 --cli-peers 127.0.0.1:10000,127.0.0.1:10002
# Terminal 3:
RUST_LOG=debug ./server --cli-port 10002 --cli-peers 127.0.0.1:10000,127.0.0.1:10001
# Terminal 4:
RUST_LOG=debug ./server --cli-port 10003 --cli-peers 127.0.0.1:10001,127.0.0.1:10002
```

## 🛠️ Development and Testing

Unit tests are made to ensure the correctness of the code; they are automatically run using the CI/CD pipeline at each commit.

### Run Unit Tests

```sh
cargo test --all-features
```

### Format the Code

```sh
cargo fmt
```

### Generate the Documentation

```sh
cargo doc
```

## 📜 License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for more details.
