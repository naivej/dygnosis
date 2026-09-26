// compare: markdown skips a dimension whose lists are empty
var y;
heterogeneity_dimension edit, quiet;
var(heterogeneity=edit) a;
var(heterogeneity=quiet) b;
model;
[name='anchor'] y = 0;
end;
model(heterogeneity=quiet);
[name='stay'] b = 1;
end;
model(heterogeneity=edit);
[name='move', bind='ELB'] a = 2;
end;
