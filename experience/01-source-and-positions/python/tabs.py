def tabbed(x):
	if x:
		return tabbed(x - 1)
	return 0
