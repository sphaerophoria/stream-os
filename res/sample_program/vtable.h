struct vtable {
	void (*print)(char *);
	void (*exit)(int);
	void (*panic)(char *);
};
