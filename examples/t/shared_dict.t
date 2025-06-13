#!/usr/bin/perl

# (C) Nginx, Inc

# Tests for ngx-rust example modules.

###############################################################################

use warnings;
use strict;

use Test::More;

BEGIN { use FindBin; chdir($FindBin::Bin); }

use lib 'lib';
use Test::Nginx;

###############################################################################

select STDERR; $| = 1;
select STDOUT; $| = 1;

my $t = Test::Nginx->new()->has(qw/http/)->plan(6)
	->write_file_expand('nginx.conf', <<'EOF');

%%TEST_GLOBALS%%

daemon off;

worker_processes 2;

events {
}

http {
    %%TEST_GLOBALS_HTTP%%

    shared_dict_zone z 64k;
    shared_dict $arg_key $foo;

    server {
        listen       127.0.0.1:8080;
        server_name  localhost;

        add_header X-Value $foo;
        add_header X-Process $pid;

        location /set/ {
            set $foo $arg_value;
            return 200;
        }
    }
}

EOF

$t->write_file('index.html', '');
$t->run();

###############################################################################

like(http_get('/set/?key=fst&value=hello'), qr/200 OK/, 'set value 1');
like(http_get('/set/?key=snd&value=world'), qr/200 OK/, 'set value 2');

ok(check('/?key=fst', qr/X-Value: hello/i), 'check value 1');
ok(check('/?key=snd', qr/X-Value: world/i), 'check value 2');

like(http_get('/set/?key=fst&value=new_value'), qr/200 OK/, 'update value 1');
ok(check('/?key=fst', qr/X-Value: new_value/i), 'check updated value');

###############################################################################

sub check {
	my ($uri, $like) = @_; 

	my $r = http_get($uri);

	return unless ($r =~ $like && $r =~ /X-Process: (\d+)/);

	return 1 if $^O eq 'MSWin32'; # only one active worker process

	my $pid = $1;

	for (1 .. 25) {
		$r = http_get($uri);
        
		return unless ($r =~ $like && $r =~ /X-Process: (\d+)/);
		return 1 if $pid != $1;
	}
}

###############################################################################
