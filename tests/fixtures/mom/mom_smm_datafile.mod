// inventory: mom_quiet_smm_datafile
// `mom_method=SMM` with a quoted `datafile`. The file need not exist for the
// check pass; a named one that is missing is W160's business.
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

method_of_moments(mom_method = SMM, datafile = 'data.csv');
