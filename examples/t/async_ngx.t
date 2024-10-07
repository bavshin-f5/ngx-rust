#!/usr/bin/perl

# (C) Nginx, Inc.

# Tests for http resolver.

###############################################################################

use warnings;
use strict;

use Test::More;

use IO::Select;
use Socket qw/ CRLF /;

BEGIN { use FindBin; chdir($FindBin::Bin); }

use lib 'lib';
use Test::Nginx qw/ :DEFAULT http_end /;

###############################################################################

select STDERR; $| = 1;
select STDOUT; $| = 1;

my $t = Test::Nginx->new()->has(qw/http proxy rewrite/);

$t->write_file_expand('nginx.conf', <<'EOF');

%%TEST_GLOBALS%%

daemon off;

events {
}

http {
    %%TEST_GLOBALS_HTTP%%

    server {
        listen       127.0.0.1:8080;
        server_name  localhost;

        location / {
            resolver    127.0.0.1:%%PORT_8981_UDP%%;
            resolver_timeout 1s;

            async_pass  127.0.0.1:%%PORT_8081%%;
        }
    }
}

EOF

$t->run_daemon(\&dns_daemon, port(8981), $t);
$t->run_daemon(\&http_daemon, port(8081));

$t->run()->plan(1);

$t->waitforfile($t->testdir . '/' . port(8981));

###############################################################################

# like(get('example.com', '/'), qr/200 OK/, 'get');

like(http_post('example.com', '/', "Hello World!"), qr/200 OK/, 'post');

###############################################################################

sub get {
	my ($host, $uri, %extra) = @_;
	return http(<<EOF, %extra);
GET $uri HTTP/1.0
Host: $host

EOF
}

sub http_post {
	my ($host, $uri, $body, %extra) = @_;
	my $len = length($body);

	return http(<<EOF, %extra);
POST $uri HTTP/1.0
Host: $host
Content-Length: $len

$body
EOF
}

###############################################################################

sub reply_handler {
	my ($recv_data, $port, $state, %extra) = @_;

	my (@name, @rdata);

	use constant NOERROR	=> 0;

	use constant A		=> 1;
	use constant CNAME	=> 5;
	use constant AAAA   => 28;

	use constant IN		=> 1;

	# default values

	my ($hdr, $rcode, $ttl) = (0x8180, NOERROR, 3600);

	# decode name

	my ($len, $offset) = (undef, 12);
	while (1) {
		$len = unpack("\@$offset C", $recv_data);
		last if $len == 0;
		$offset++;
		push @name, unpack("\@$offset A$len", $recv_data);
		$offset += $len;
	}

	$offset -= 1;
	my ($id, $type, $class) = unpack("n x$offset n2", $recv_data);

	my $name = join('.', @name);
	if (($name eq 'example.com')) {
		if ($type == A || $type == CNAME) {
			push @rdata, rd_addr($ttl, '127.0.0.1');
		}
		if ($type = AAAA) {
			push @rdata, rd_addr6($ttl, "::1");
		}
	}

	$len = @name;
	pack("n6 (C/a*)$len x n2", $id, $hdr | $rcode, 1, scalar @rdata,
		0, 0, @name, $type, $class) . join('', @rdata);
}

sub rd_addr {
	my ($ttl, $addr) = @_;

	my $code = 'split(/\./, $addr)';

	pack 'n3N nC4', 0xc00c, A, IN, $ttl, eval "scalar $code", eval($code);
}

sub expand_ip6 {
    my ($addr) = @_;

    substr ($addr, index($addr, "::"), 2) =
        join "0", map { ":" } (0 .. 8 - (split /:/, $addr) + 1);
    map { hex "0" x (4 - length $_) . "$_" } split /:/, $addr;
}

sub rd_addr6 {
    my ($ttl, $addr) = @_;

    pack 'n3N nn8', 0xc00c, AAAA, IN, $ttl, 16, expand_ip6($addr);
}

sub dns_daemon {
	my ($port, $t, %extra) = @_;

	my ($data, $recv_data);
	my $socket = IO::Socket::INET->new(
		LocalAddr => '127.0.0.1',
		LocalPort => $port,
		Proto => 'udp',
	)
		or die "Can't create listening socket: $!\n";

	my $sel = IO::Select->new($socket);
	my $tcp = 0;

	if ($extra{tcp}) {
		$tcp = port(8983, socket => 1);
		$sel->add($tcp);
	}

	local $SIG{PIPE} = 'IGNORE';

	# track number of relevant queries

	my %state = (
		cnamecnt	=> 0,
		twocnt		=> 0,
		ttlcnt		=> 0,
		ttl0cnt		=> 0,
		cttlcnt		=> 0,
		cttl2cnt	=> 0,
		manycnt		=> 0,
		casecnt		=> 0,
		idcnt		=> 0,
		fecnt		=> 0,
	);

	# signal we are ready

	open my $fh, '>', $t->testdir() . '/' . $port;
	close $fh;

	while (my @ready = $sel->can_read) {
		foreach my $fh (@ready) {
			if ($tcp == $fh) {
				my $new = $fh->accept;
				$new->autoflush(1);
				$sel->add($new);

			} elsif ($socket == $fh) {
				$fh->recv($recv_data, 65536);
				$data = reply_handler($recv_data, $port,
					\%state);
				$fh->send($data);

			} else {
				$fh->recv($recv_data, 65536);
				unless (length $recv_data) {
					$sel->remove($fh);
					$fh->close;
					next;
				}

again:
				my $len = unpack("n", $recv_data);
				$data = substr $recv_data, 2, $len;
				$data = reply_handler($data, $port, \%state,
					tcp => 1);
				$data = pack("n", length $data) . $data;
				$fh->send($data);
				$recv_data = substr $recv_data, 2 + $len;
				goto again if length $recv_data;
			}
		}
	}
}

sub http_daemon {
	my ($port) = @_;
	my $server = IO::Socket::INET->new(
		Proto => 'tcp',
		LocalHost => '127.0.0.1',
		LocalPort => $port,
		Listen => 5,
		Reuse => 1
	)
		or die "Can't create listening socket: $!\n";

	local $SIG{PIPE} = 'IGNORE';

	while (my $client = $server->accept()) {
		$client->autoflush(1);

		my $headers = '';

		while (<$client>) {
			$headers .= $_;
			last if (/^\x0d?\x0a?$/);
		}

		my $body = "TEST-OK-IF-YOU-SEE-THIS";

		my $p = "HTTP/1.1 200 OK" . CRLF .
			"Connection: close" . CRLF .
			"Content-Length: " . length($body) . CRLF . CRLF;

		print $client $p;
		print $client $body unless $headers =~ /^HEAD/i;

		close $client;
	}
}

###############################################################################
