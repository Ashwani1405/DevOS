---
title: ZippyDB What Actually Happens Behind PUT(key, value)
slug: zippydb-what-actually-happens-behind-put
published: true
kind: blog
date: 2026-09-14
description: "What actually happens inside ZippyDB when you call PUT(key, value) — sharding, μshards, replication, Paxos, Multi-Paxos, consistency and durability."
tags:
  - distributed systems
readingTime: 10 min
---

I was reading about ZippyDB today and initially it kind of looked like another distributed key-value store, which it kind of is.

But then when you look at what is actually going on inside it, you realise there is a lot more going on than just `get(key)` and `put(key, value)`.

**ZippyDB** is Facebook's general-purpose strongly consistent distributed key-value store. It has been around since 2013 and is built on top of RocksDB, and this is especially the part which I found interesting.

Before ZippyDB, different teams at Facebook were using RocksDB directly. And every team was basically running into the same problems:

- replication
    
- failure recovery
    
- consistency
    
- fault tolerance
    
- capacity management
    

So instead of every team solving these separately, they built a layer around RocksDB that handled all of this.

I think that was one of the best moves: introduce a generalized layer instead of making every team reinvent the same distributed-systems machinery.

ZippyDB is basically that layer.

But obviously, the actual system gets much more complicated once you have to run it across regions and at Facebook scale.

The architecture is mostly a combination of existing pieces:

- **RocksDB** handles storage.
    
- **Data Shuttle** handles replication.
    
- **ShardManager** handles shard placement, load balancing and failure detection.
    
- **ZooKeeper** is used for distributed configuration.


![[Pasted image 20260914165840.png]]
    

And then ZippyDB puts these things together into an actual managed key-value store.

Yeah, I like this approach because they weren't trying to reinvent every component, which is very redundant in itself.

---

## The interesting part starts when you look at how the data is divided

So if we look at it, ZippyDB has **shards**.

A shard is basically the unit of data management on the server side.

But applications don't actually work directly with these physical shards.

Instead, there is another layer called a **μshard**, or micro-shard.

This confused me a little initially because I was thinking, why do we need another layer?

And even you will have the same question lol.

A physical shard can be around **50–100 GB** and can contain tens of thousands of μshards.

So instead of making the physical shard the thing the application works with, the application partitions its key space into these smaller μshards.

This gives ZippyDB some freedom to move things around underneath the client.

So if a physical shard becomes too large or too hot, ZippyDB can basically rearrange the μshards without the client having to know that anything happened.

### There are two ways ZippyDB maps μshards to physical shards.

**1. Compact mapping**

This is the simpler one and doesn't change very often.

It is mostly changed when a shard needs to be split because it has become too large or too hot.

**2. Akkio**

Then there is Akkio, where it manages the μshards and can place them in regions where the data is actually being accessed.

This is kinda useful because otherwise you might just replicate everything everywhere to get low-latency reads.

Which obviously works, but now you're storing the same data everywhere and I don't think anyone possibly wants that.

So Akkio tries to avoid that by putting the data closer to where it is actually needed.
![[Pasted image 20260914165916.png]]
---

## So now we have a shard, but it can't just live on one machine

If that machine goes down, the data goes down with it.

So ZippyDB replicates shards across multiple machines and potentially multiple regions.
![[Pasted image 20260914165932.png]]

One replica acts as the **primary/leader**.

A subset of those replicas participate in synchronous replication and form the **Paxos quorum group**, while the remaining replicas can be followers that receive the data asynchronously.

And this distinction is actually pretty useful.

You don't necessarily want every replica participating in the synchronous quorum.

You can keep the quorum relatively small so writes don't have to wait on a huge number of machines, while still having additional followers around different regions for low-latency reads.

So you get something like:

![[Pasted image 20260914165950.png]]

A, B and C might be the replicas participating in the Paxos quorum while D is just a follower catching up asynchronously.

So you can have a small quorum for writes while still having a bunch of replicas around the world for reads.

The obvious tradeoff is that those follower reads can be stale.

But you don't have to make every replica part of the synchronous write path just to get good read locality.

---

## And now the interesting question is: what actually happens when a write happens?

one of the replicas acts as the leader for the shard. the leader is the one coordinating writes, but it isn't permanently the leader for that ZippyDB divides time into epochs, and each epoch has a leader and ShardManager assigns that leader with a lease for the duration of the epoch. It keeps that lease alive through heartbeats.

![[Pasted image 20260914170006.png]]

The epoch is basically the generation of the current leadership.

And this matters, to be honest, because a distributed system can't assume that the old leader immediately knows that it isn't the leader anymore.

---

## Now we have a leader, but we still need the replicas to agree on what happened and in what order

The leader gives every write a monotonically increasing sequence number:

```text
1 → PUT x=10
2 → PUT y=20
3 → DELETE z
4 → PUT x=30
```

So now there is a total ordering of writes within the shard.

But the leader can't just decide that this is the order and expect every other replica to blindly follow it.

The replicas in the quorum need to agree on it.

---

## AND that's where Paxos comes in

The writes go into a replicated durable log and **Multi-Paxos** is used to get the quorum replicas to agree on the entries in that log.

The easiest way to think about this is:

> Every write needs a place in the log, and the replicas need to agree on what goes into that place.

Say the leader has already assigned:

```text
41 → PUT x=10
42 → PUT x=20
43 → DELETE x
```

For entry 42, the leader proposes to the Paxos replicas:

```text
slot 42 = PUT x=20
```


![[Pasted image 20260914170035.png]]The replicas in the quorum now need to agree that `PUT x=20` is the value for slot 42.

Once a quorum agrees, that entry is decided and can become part of the replicated log.

So the log starts looking something like:

```text
41 → PUT x=10
42 → PUT x=20
43 → DELETE x
44 → PUT y=50
```

And the important thing is that the quorum replicas agree on the same entries at the same positions.
Now imagine doing this independently for every single write.

For every new slot we'd have to go through the leader-establishment part again.
![[Pasted image 20260914170051.png]]

---

## And that's where Multi-Paxos comes in

Once a leader has been established, we can keep that leader for multiple log entries instead of repeating the leader-establishment phase for every slot.

So rather than electing a leader every time:
we do
![[Pasted image 20260914170127.png]]
the same leader keeps driving consensus for successive slots. and this doesn't mean leader election has disappeared.

> if that leader fails, or ShardManager decides the primary needs to change, we get a new epoch and a new leader. then the new leader can continue driving consensus for the following entries.

so Multi-Paxos is basically Paxos being used repeatedly for a sequence of log entries while reusing the same leader. The expensive leader-establishment part happens when leadership changes, rather than once for every write.

---

## And now the write path will make more sense

```text
client
  ↓
leader
  ↓
sequence number
  ↓
log slot
  ↓
Multi-Paxos
  ↓
quorum agrees
  ↓
replicated log
  ↓
replicas apply in order
```

---

## So now we have a replicated log

So now we have a replicated log and the quorum replicas agree on the entries in it.

But there's still something interesting here.

The replicas don't necessarily apply those entries at exactly the same time.

For example, say the log currently looks like:

![[Pasted image 20260914170232.png]]

so now the question becomes: if a read goes to that follower, what are we okay with returning? and this is where ZippyDB gives the application different consistency options.

---

## The simplest one is eventual consistency

Basically, you're saying:

> I don't care if this replica is a little behind, just give me the value from here.

but ZippyDB puts a bound on how behind that replica is allowed to be. It already knows the ordering of writes, and heartbeats are used to figure out how far a replica has fallen behind.

so if the primary is at sequence 45, a follower at 43 might be fine, but if some follower is way behind, ZippyDB won't serve the read from it.

so it's technically called eventual consistency, but it's actually closer to bounded staleness than the usual idea of "eventually it'll catch up".

---

## Then there's read-your-writes

This one is interesting because you don't necessarily need to go back to the primary.

Say I write:

```text
PUT x = 20
```

And the server tells me that this write got sequence number 42.

The client remembers that 42.

Now if I immediately do a read, it can basically say:

> Give me x at sequence >= 42

So if a follower is still at 39, it can't satisfy that read.

But another follower that's already at 42 can.

So you can read locally from a **follower** and get something slightly stale, but there's a configurable bound on how far behind that replica is allowed to be.

Which makes this feel a lot closer to **bounded staleness** than the completely unconstrained eventual consistency you might initially imagine.
![[Pasted image 20260914170313.png]]

And I like this because the client doesn't need to say:

> always read from the primary after I write.

It just carries the version it needs.

**ZippyDB does this by having the client cache the latest sequence number returned by the server and use that for at-or-later reads.**

---

## And then finally there's strong consistency or linearizability

Here you basically don't want to take any chances with a stale follower, so the read goes to the primary.

The interesting part is that the primary has a lease for the current epoch.

So before serving the read, it can rely on that lease to know that it is still the valid primary and that some newer primary hasn't taken over.

Normally that means you don't need another quorum round trip for every strong read.

If the primary is in some weird situation where it hasn't heard back about its lease renewal and isn't sure that it's still valid, the read can fall back to a quorum check.
![[Pasted image 20260914170350.png]]

So now the system has three pretty different ways of answering the same `GET`.


And this is probably one of the more useful design choices in ZippyDB because the application gets to make this tradeoff **per request**, instead of the whole database having to pick one consistency model
![[Pasted image 20260914170356.png]]

---

## There's another tradeoff here which is easy to mix up with consistency

### When does ZippyDB actually consider a write durable?

By default, a write isn't acknowledged just because the primary received it.

The data is persisted in the Paxos logs of a majority of the quorum replicas, and the primary also writes the data to RocksDB before sending the ACK back to the client.

So roughly:

![[Pasted image 20260914170410.png]]

The important part is that the ACK happens **after the write has reached a majority of the Paxos replicas**, rather than just after the primary has received it.

So if the primary dies right after sending the ACK, the write isn't only sitting on that one machine.

The quorum already has it in its durable log.

But obviously, waiting for replicas to persist the write, especially when those replicas can be in different regions, adds latency.

---

## So ZippyDB also has a fast-acknowledge mode where that latency matters

In that mode, the primary acknowledges the write as soon as it has been enqueued for replication.

So yeah, it's faster.

But you're giving up some durability and consistency at the cost.

---

## And honestly, I think this is a pretty good place to stop the deep dive

Because at this point we started with something that looked like:

```text
GET(key)
PUT(key, value)
```

And ended up going through:

- sharding
    
- μshards
    
- replication
    
- quorum
    
- followers
    
- epochs
    
- leases
    
- ordered logs
    
- Paxos
    
- Multi-Paxos
    
- consistency
    
- durability
    

Just to make that stupid little `PUT` actually work reliably at Facebook scale lmaooooo.

And let me be honest, that's probably the part I found most interesting about ZippyDB.

The key-value interface is extremely boring.

Which is probably exactly what you want.

You don't want every application team at Facebook thinking about Paxos, shard placement, replica lag, failure detection, leadership leases and recovery every time they need to store some metadata.

They just want a key and a value.

> **All the distributed systems complexity is hidden underneath.**

**peace....**