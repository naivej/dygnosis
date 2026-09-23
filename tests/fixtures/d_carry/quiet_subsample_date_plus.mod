// inventory: d_carry_quiet_subsample_date_plus
var y; varexo e; parameters a; a=.5;
model; y=a*y(-1)+e; end;
a.subsamples(s=2000Q1:2001Q1+1);
