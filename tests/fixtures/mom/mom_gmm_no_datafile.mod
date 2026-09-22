// inventory: mom_e383_gmm_no_datafile
// `mom_method=GMM` without a `datafile`. 7.1's checkPass refuses: GMM matches
// moments against data.
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

method_of_moments(mom_method = GMM);
