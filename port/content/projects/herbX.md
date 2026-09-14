---

title: herbX

slug: herbx

tagline: Traceable Ayurvedic herb supply chains using blockchain and AI.

description: A permissioned Hyperledger Fabric platform for immutable herb provenance, smart-contract workflows, and AI-assisted herb classification.

status: archived

year: "2025"

tags:

- blockchain

- supply-chain

- provenance

- AI

stack:

- Hyperledger Fabric

- Smart Contracts

- Go

- Node.js

- Express

github: https://github.com/Yashbhu/herbX

featured: true

---

  

## Overview

  

herbX is a permissioned blockchain platform for improving traceability and authenticity across the Ayurvedic herb supply chain.

  

The system records lifecycle events on Hyperledger Fabric and uses smart contracts to enforce workflow rules between collectors, processors, laboratories, manufacturers, regulators, and other authorized participants.

  

## What it does

  

- Registers herb batches and their origin.

- Tracks processing, grading, transport, and storage.

- Records laboratory testing and quality reports.

- Verifies provenance before manufacturing.

- Provides tamper-resistant audit history.

- Uses smart contracts for custody transfers and workflow enforcement.

- Integrates AI-based herb classification.

- Provides cryptographic digital certificates.

  

## Lifecycle

  

```text

Collector

↓

Batch Registration

↓

Processing / Grading

↓

Transport / Storage

↓

Laboratory Verification

↓

Manufacturing

↓

Traceability Verification

```

  

## Repository structure

  

```text

chaincode/ Smart contracts

api/ Backend API

network/ Fabric network configuration

scripts/ Setup and automation

docs/ Technical documentation

tests/ Unit and integration tests

```