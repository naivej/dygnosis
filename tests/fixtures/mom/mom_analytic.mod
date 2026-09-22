// inventory: mom_e384_analytic_no_gmm
// `analytic_standard_errors` with `mom_method=IRF_MATCHING`. 7.1's checkPass
// refuses: only GMM has the analytic derivatives this option asks for.
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

method_of_moments(mom_method = IRF_MATCHING, analytic_standard_errors);
