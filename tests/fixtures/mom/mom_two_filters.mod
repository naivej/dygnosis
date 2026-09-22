// inventory: mom_e385_two_filters
// Two of `hp_filter`, `one_sided_hp_filter`, and `bandpass_filter` on one
// statement. 7.1's checkPass refuses: only one filter may be used.
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

method_of_moments(mom_method = IRF_MATCHING, hp_filter = 1600, bandpass_filter);
