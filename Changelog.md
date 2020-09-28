# Change Log

## Unreleased next version

Breaking Changes

* `pdu::Error` has been changed to contain an allocated octets buffer
  instead of being generic in a very fragile way. Consequently,
  `pdu::BoxedError` has been dropped. ([#6])
* The client traits `VrpTarget` and `VrpUpdate` have been modified to
  allow returning errors when processing data. ([#7])
* The minimum supported Rust version is now 1.42. ([#6])

Bug Fixes

* The server now correctly responds to received error PDUs by returning an
  error from `server::Connection::run`. ([#6])

New

* Implemented `Default` for `state::Serial` [(#4])
* The client now provides a method `Client::update` that can be used to
  fetch a single update instead of just letting it run forever. ([#7])


Other Changes

[#4]: https://github.com/NLnetLabs/rpki-rtr/pull/4
[#6]: https://github.com/NLnetLabs/rpki-rtr/pull/6
[#7]: https://github.com/NLnetLabs/rpki-rtr/pull/7


## 0.1.1

Dependencies

* Downgrades dependencies to the lowest versions actually required:
  futures-util 0.3, log 0.4.4., and tokio 0.2.11. ([#1], [#2])

[#1]: https://github.com/NLnetLabs/rpki-rtr/pull/1
[#2]: https://github.com/NLnetLabs/rpki-rtr/pull/2


## 0.1.0

Initial release.

