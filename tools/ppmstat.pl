use strict;
use warnings;

my $path = shift @ARGV or die "usage: ppmstat.pl file.ppm\n";
open(my $fh, "<", $path) or die "$path: $!";
binmode $fh;
my $magic = <$fh>;
die "not P6" unless defined $magic && $magic =~ /^P6/;
my $line = <$fh>;
$line = <$fh> if $line =~ /^\s*#/;
my ($w, $h) = split(/\s+/, $line);
my $max = <$fh>;
my $data = "";
read($fh, $data, $w * $h * 3);
my %hist;
for my $i (0 .. $w * $h - 1) {
    my $px = substr($data, $i * 3, 3);
    $hist{$px}++;
}
my $n = 0;
for my $k (sort { $hist{$b} <=> $hist{$a} } keys %hist) {
    my ($r, $g, $b) = unpack("C3", $k);
    printf "%02x%02x%02x %d\n", $r, $g, $b, $hist{$k};
    last if ++$n >= 10;
}
print "corner pixels:\n";
for my $y (0 .. 3) {
    my @row;
    for my $x (0 .. 15) {
        my ($r, $g, $b) = unpack("C3", substr($data, ($y * $w + $x) * 3, 3));
        push @row, sprintf("%02x%02x%02x", $r, $g, $b);
    }
    print join(" ", @row), "\n";
}
